<?php

namespace App\Services;

use App\Exceptions\BackupJobCancelledException;
use Illuminate\Support\Facades\Crypt;
use Illuminate\Support\Facades\File;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Storage;
use Illuminate\Support\Str;
use Laravel\Socialite\Facades\Socialite;

class GoogleDriveBackupService
{
    public const CREDENTIALS_PATH = 'google-drive/credentials.json';

    public const JOB_DIR = 'google-drive-upload-jobs';

    public const FOLDER_NAME = 'Arumanis Backups';

    /** Chunk size must be a multiple of 256 KiB for Drive resumable upload. */
    private const CHUNK_BYTES = 8 * 1024 * 1024;

    private const DRIVE_SCOPE = 'https://www.googleapis.com/auth/drive.file';

    public function __construct(
        private readonly SystemBackupService $backups,
    ) {}

    public function redirectUri(): string
    {
        $configured = trim((string) config('services.google.drive_redirect', ''));
        if ($configured !== '') {
            return $configured;
        }

        return rtrim((string) config('app.url'), '/').'/api/app-settings/backups/google-drive/callback';
    }

    public function isConfigured(): bool
    {
        return filled(config('services.google.client_id'))
            && filled(config('services.google.client_secret'));
    }

    public function status(): array
    {
        $creds = $this->readCredentials();

        return [
            'configured' => $this->isConfigured(),
            'connected' => filled($creds['refresh_token'] ?? null),
            'email' => $creds['email'] ?? null,
            'folder_id' => $creds['folder_id'] ?? null,
            'folder_name' => self::FOLDER_NAME,
            'connected_at' => $creds['connected_at'] ?? null,
        ];
    }

    public function buildConnectUrl(): string
    {
        abort_unless($this->isConfigured(), 422, 'Google OAuth belum dikonfigurasi (GOOGLE_CLIENT_ID / SECRET)');

        $stateToken = Str::random(40);
        cache()->put("google_drive_oauth_state:{$stateToken}", [
            'created_at' => now()->toIso8601String(),
        ], now()->addMinutes(15));

        /** @var \Laravel\Socialite\Two\AbstractProvider $driver */
        $driver = Socialite::driver('google');

        return $driver
            ->scopes([self::DRIVE_SCOPE, 'openid', 'profile', 'email'])
            ->stateless()
            ->redirectUrl($this->redirectUri())
            ->with([
                'access_type' => 'offline',
                'prompt' => 'consent',
                'state' => $stateToken,
            ])
            ->redirect()
            ->getTargetUrl();
    }

    public function handleCallback(string $code, ?string $state): array
    {
        abort_unless($this->isConfigured(), 422, 'Google OAuth belum dikonfigurasi');

        if (! filled($state) || ! cache()->pull("google_drive_oauth_state:{$state}")) {
            throw new \RuntimeException('State OAuth Google Drive tidak valid atau kedaluwarsa');
        }

        /** @var \Laravel\Socialite\Two\AbstractProvider $driver */
        $driver = Socialite::driver('google');
        $googleUser = $driver
            ->stateless()
            ->redirectUrl($this->redirectUri())
            ->user();

        $refreshToken = $googleUser->refreshToken;
        if (! filled($refreshToken)) {
            // Re-consent sometimes returns only access token; keep previous refresh if any.
            $existing = $this->readCredentials();
            $refreshToken = $existing['refresh_token'] ?? null;
        }

        if (! filled($refreshToken)) {
            throw new \RuntimeException(
                'Google tidak mengembalikan refresh token. Cabut akses aplikasi di https://myaccount.google.com/permissions lalu hubungkan ulang.'
            );
        }

        $expiresIn = (int) ($googleUser->expiresIn ?? 3600);

        $this->writeCredentials([
            'refresh_token' => $refreshToken,
            'access_token' => $googleUser->token,
            'access_token_expires_at' => now()->addSeconds(max(60, $expiresIn - 60))->toIso8601String(),
            'email' => $googleUser->getEmail(),
            'folder_id' => null,
            'connected_at' => now()->toIso8601String(),
        ]);

        // Ensure backup folder exists so status can show readiness.
        try {
            $this->ensureBackupFolder();
        } catch (\Throwable $e) {
            Log::warning('Google Drive folder ensure failed after connect', [
                'message' => $e->getMessage(),
            ]);
        }

        return $this->status();
    }

    public function disconnect(): void
    {
        if (Storage::disk('local')->exists(self::CREDENTIALS_PATH)) {
            Storage::disk('local')->delete(self::CREDENTIALS_PATH);
        }
    }

    public function queueUpload(string $filename): array
    {
        $this->backups->guardFilename($filename);
        $absolute = $this->backups->backupAbsolutePath($filename);
        abort_unless(File::exists($absolute), 404, 'Backup tidak ditemukan');

        $status = $this->status();
        abort_unless($status['connected'], 422, 'Google Drive belum terhubung. Hubungkan dulu di Pengaturan.');

        $jobId = (string) Str::uuid();
        $this->writeJobStatus($jobId, [
            'job_id' => $jobId,
            'status' => 'queued',
            'filename' => $filename,
            'size' => File::size($absolute),
            'created_at' => now()->toIso8601String(),
            'message' => 'Upload ke Google Drive masuk antrean',
            'progress' => 0,
        ]);

        $detached = $this->dispatchDetached($jobId, $filename);

        if (! $detached) {
            app()->terminating(function () use ($jobId, $filename) {
                $this->runUploadJob($jobId, $filename);
            });
        }

        return $this->readJobStatus($jobId) ?? [
            'job_id' => $jobId,
            'status' => 'queued',
            'filename' => $filename,
            'message' => 'Upload ke Google Drive masuk antrean',
        ];
    }

    public function runUploadJob(string $jobId, string $filename): void
    {
        ignore_user_abort(true);
        @set_time_limit(0);
        @ini_set('memory_limit', '512M');

        $this->backups->guardJobId($jobId);
        $this->backups->guardFilename($filename);

        $absolute = $this->backups->backupAbsolutePath($filename);
        $initial = $this->readJobStatus($jobId) ?? [];

        $this->recordWorkerPid($jobId, getmypid());

        if ($this->isCancelRequested($jobId)) {
            $this->finalizeCancelledJob($jobId, $filename, $initial);

            return;
        }

        $this->writeJobStatus($jobId, [
            'job_id' => $jobId,
            'status' => 'running',
            'filename' => $filename,
            'size' => File::exists($absolute) ? File::size($absolute) : ($initial['size'] ?? 0),
            'created_at' => $initial['created_at'] ?? now()->toIso8601String(),
            'started_at' => now()->toIso8601String(),
            'message' => 'Mengunggah ke Google Drive',
            'progress' => 1,
        ]);

        try {
            if (! File::exists($absolute)) {
                throw new \RuntimeException('File backup tidak ditemukan di server');
            }

            $result = $this->uploadFileResumable($absolute, $filename, $jobId);

            $running = $this->readJobStatus($jobId) ?? [];
            $this->writeJobStatus($jobId, [
                'job_id' => $jobId,
                'status' => 'completed',
                'filename' => $filename,
                'size' => File::size($absolute),
                'created_at' => $running['created_at'] ?? now()->toIso8601String(),
                'started_at' => $running['started_at'] ?? null,
                'finished_at' => now()->toIso8601String(),
                'message' => 'Upload ke Google Drive berhasil',
                'progress' => 100,
                'result' => $result,
            ]);
        } catch (BackupJobCancelledException $exception) {
            $running = $this->readJobStatus($jobId) ?? [];
            $this->finalizeCancelledJob($jobId, $filename, $running);
        } catch (\Throwable $exception) {
            if ($this->isCancelRequested($jobId)) {
                $running = $this->readJobStatus($jobId) ?? [];
                $this->finalizeCancelledJob($jobId, $filename, $running);

                return;
            }

            Log::error('Google Drive upload job failed', [
                'job_id' => $jobId,
                'filename' => $filename,
                'exception' => $exception,
            ]);

            $running = $this->readJobStatus($jobId) ?? [];
            $this->writeJobStatus($jobId, [
                'job_id' => $jobId,
                'status' => 'failed',
                'filename' => $filename,
                'created_at' => $running['created_at'] ?? now()->toIso8601String(),
                'started_at' => $running['started_at'] ?? null,
                'finished_at' => now()->toIso8601String(),
                'message' => 'Upload ke Google Drive gagal',
                'progress' => 0,
                'error' => $exception->getMessage(),
            ]);
        }
    }

    public function readJobStatus(string $jobId): ?array
    {
        $this->backups->guardJobId($jobId);
        $path = $this->jobFilePath($jobId);
        if (! Storage::disk('local')->exists($path)) {
            return null;
        }

        $status = json_decode(Storage::disk('local')->get($path), true);

        return is_array($status) ? $status : null;
    }

    public function cancelUploadJob(string $jobId): array
    {
        $status = $this->readJobStatus($jobId);
        abort_unless($status !== null, 404, 'Status upload tidak ditemukan');

        $state = (string) ($status['status'] ?? '');
        if (in_array($state, ['completed', 'failed', 'cancelled'], true)) {
            return $status;
        }

        $this->writeJobStatus($jobId, array_merge($status, [
            'cancel_requested' => true,
            'cancel_requested_at' => now()->toIso8601String(),
            'message' => 'Membatalkan upload…',
        ]));

        SystemBackupService::terminateProcessPid(isset($status['pid']) ? (int) $status['pid'] : null);

        return $this->readJobStatus($jobId) ?? $status;
    }

    public function recordWorkerPid(string $jobId, int $pid): void
    {
        $current = $this->readJobStatus($jobId) ?? [];
        $this->writeJobStatus($jobId, array_merge($current, [
            'job_id' => $jobId,
            'pid' => $pid,
        ]));
    }

    private function isCancelRequested(string $jobId): bool
    {
        $status = $this->readJobStatus($jobId);

        return (bool) ($status['cancel_requested'] ?? false);
    }

    private function assertNotCancelled(string $jobId): void
    {
        if ($this->isCancelRequested($jobId)) {
            throw new BackupJobCancelledException('Upload ke Google Drive dibatalkan');
        }
    }

    private function finalizeCancelledJob(string $jobId, string $filename, array $previousStatus): void
    {
        $this->writeJobStatus($jobId, [
            'job_id' => $jobId,
            'status' => 'cancelled',
            'filename' => $filename,
            'size' => $previousStatus['size'] ?? null,
            'created_at' => $previousStatus['created_at'] ?? now()->toIso8601String(),
            'started_at' => $previousStatus['started_at'] ?? null,
            'finished_at' => now()->toIso8601String(),
            'message' => 'Upload ke Google Drive dibatalkan',
            'progress' => 0,
            'cancel_requested' => true,
        ]);
    }

    /**
     * @return array{id: string, name: string, webViewLink: string|null, size: int|null}
     */
    private function uploadFileResumable(string $absolutePath, string $filename, string $jobId): array
    {
        $folderId = $this->ensureBackupFolder();
        $accessToken = $this->getAccessToken();
        $fileSize = File::size($absolutePath);

        $init = Http::withToken($accessToken)
            ->withHeaders([
                'Content-Type' => 'application/json; charset=UTF-8',
                'X-Upload-Content-Type' => 'application/zip',
                'X-Upload-Content-Length' => (string) $fileSize,
            ])
            ->post(
                'https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&fields=id,name,webViewLink,size',
                [
                    'name' => $filename,
                    'parents' => [$folderId],
                ]
            );

        if (! $init->successful()) {
            throw new \RuntimeException(
                'Gagal memulai upload Drive: '.($init->json('error.message') ?? $init->body())
            );
        }

        $sessionUri = $init->header('Location');
        if (! filled($sessionUri)) {
            throw new \RuntimeException('Google Drive tidak mengembalikan session upload');
        }

        $handle = fopen($absolutePath, 'rb');
        if ($handle === false) {
            throw new \RuntimeException('Gagal membuka file backup untuk diunggah');
        }

        try {
            $offset = 0;
            $finalBody = null;
            $authRetries = 0;

            while ($offset < $fileSize) {
                $this->assertNotCancelled($jobId);

                $chunk = fread($handle, self::CHUNK_BYTES);
                if ($chunk === false || $chunk === '') {
                    throw new \RuntimeException('Gagal membaca chunk file backup');
                }

                $chunkLen = strlen($chunk);
                $end = $offset + $chunkLen - 1;
                $range = "bytes {$offset}-{$end}/{$fileSize}";

                $response = Http::withBody($chunk, 'application/zip')
                    ->withHeaders([
                        'Content-Length' => (string) $chunkLen,
                        'Content-Range' => $range,
                    ])
                    ->timeout(600)
                    ->put($sessionUri);

                $statusCode = $response->status();

                // 308 Resume Incomplete — more chunks needed
                if ($statusCode === 308) {
                    $offset = $end + 1;
                    $authRetries = 0;
                    $progress = (int) min(99, max(1, floor(($offset / max(1, $fileSize)) * 100)));
                    $this->patchJobProgress($jobId, $progress, "Mengunggah… {$progress}%");
                    continue;
                }

                if ($statusCode >= 200 && $statusCode < 300) {
                    $finalBody = $response->json();
                    $offset = $fileSize;
                    break;
                }

                // Access token may expire mid-upload; retry chunk once after refresh.
                if (in_array($statusCode, [401, 403], true) && $authRetries < 2) {
                    $authRetries++;
                    $this->getAccessToken(forceRefresh: true);
                    fseek($handle, $offset);
                    continue;
                }

                throw new \RuntimeException(
                    'Upload Drive gagal (HTTP '.$statusCode.'): '.($response->json('error.message') ?? $response->body())
                );
            }
        } finally {
            fclose($handle);
        }

        if (! is_array($finalBody) || empty($finalBody['id'])) {
            throw new \RuntimeException('Upload Drive selesai tanpa metadata file');
        }

        return [
            'id' => (string) $finalBody['id'],
            'name' => (string) ($finalBody['name'] ?? $filename),
            'webViewLink' => isset($finalBody['webViewLink']) ? (string) $finalBody['webViewLink'] : $this->fileWebLink((string) $finalBody['id']),
            'size' => isset($finalBody['size']) ? (int) $finalBody['size'] : $fileSize,
            'folder_id' => $folderId,
        ];
    }

    private function fileWebLink(string $fileId): string
    {
        return 'https://drive.google.com/file/d/'.$fileId.'/view';
    }

    private function ensureBackupFolder(): string
    {
        $creds = $this->readCredentials();
        if (filled($creds['folder_id'] ?? null)) {
            return (string) $creds['folder_id'];
        }

        $accessToken = $this->getAccessToken();
        $escapedName = str_replace("'", "\\'", self::FOLDER_NAME);
        $query = "name = '{$escapedName}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false";

        $list = Http::withToken($accessToken)
            ->get('https://www.googleapis.com/drive/v3/files', [
                'q' => $query,
                'spaces' => 'drive',
                'fields' => 'files(id,name)',
                'pageSize' => 1,
            ]);

        if ($list->successful()) {
            $files = $list->json('files') ?? [];
            if (is_array($files) && isset($files[0]['id'])) {
                $folderId = (string) $files[0]['id'];
                $creds['folder_id'] = $folderId;
                $this->writeCredentials($creds);

                return $folderId;
            }
        }

        $create = Http::withToken($accessToken)
            ->post('https://www.googleapis.com/drive/v3/files', [
                'name' => self::FOLDER_NAME,
                'mimeType' => 'application/vnd.google-apps.folder',
            ]);

        if (! $create->successful() || ! filled($create->json('id'))) {
            throw new \RuntimeException(
                'Gagal membuat folder Drive: '.($create->json('error.message') ?? $create->body())
            );
        }

        $folderId = (string) $create->json('id');
        $creds['folder_id'] = $folderId;
        $this->writeCredentials($creds);

        return $folderId;
    }

    private function getAccessToken(bool $forceRefresh = false): string
    {
        $creds = $this->readCredentials();
        abort_unless(filled($creds['refresh_token'] ?? null), 422, 'Google Drive belum terhubung');

        $expiresAt = $creds['access_token_expires_at'] ?? null;
        $hasValidAccess = filled($creds['access_token'] ?? null)
            && filled($expiresAt)
            && now()->lt(\Carbon\Carbon::parse($expiresAt));

        if (! $forceRefresh && $hasValidAccess) {
            return (string) $creds['access_token'];
        }

        $response = Http::asForm()->post('https://oauth2.googleapis.com/token', [
            'client_id' => config('services.google.client_id'),
            'client_secret' => config('services.google.client_secret'),
            'refresh_token' => $creds['refresh_token'],
            'grant_type' => 'refresh_token',
        ]);

        if (! $response->successful() || ! filled($response->json('access_token'))) {
            throw new \RuntimeException(
                'Gagal refresh token Google Drive. Hubungkan ulang akun. '.
                ($response->json('error_description') ?? $response->json('error') ?? '')
            );
        }

        $expiresIn = (int) ($response->json('expires_in') ?? 3600);
        $creds['access_token'] = $response->json('access_token');
        $creds['access_token_expires_at'] = now()->addSeconds(max(60, $expiresIn - 60))->toIso8601String();

        // Google may rotate refresh token.
        if (filled($response->json('refresh_token'))) {
            $creds['refresh_token'] = $response->json('refresh_token');
        }

        $this->writeCredentials($creds);

        return (string) $creds['access_token'];
    }

    private function readCredentials(): array
    {
        if (! Storage::disk('local')->exists(self::CREDENTIALS_PATH)) {
            return [];
        }

        try {
            $payload = Storage::disk('local')->get(self::CREDENTIALS_PATH);
            $json = Crypt::decryptString($payload);
            $data = json_decode($json, true);

            return is_array($data) ? $data : [];
        } catch (\Throwable $e) {
            Log::warning('Failed to read Google Drive credentials', ['message' => $e->getMessage()]);

            return [];
        }
    }

    private function writeCredentials(array $data): void
    {
        Storage::disk('local')->makeDirectory(dirname(self::CREDENTIALS_PATH));
        $encrypted = Crypt::encryptString(json_encode($data, JSON_UNESCAPED_UNICODE));
        Storage::disk('local')->put(self::CREDENTIALS_PATH, $encrypted);
    }

    private function writeJobStatus(string $jobId, array $status): void
    {
        Storage::disk('local')->makeDirectory(self::JOB_DIR);
        Storage::disk('local')->put(
            $this->jobFilePath($jobId),
            json_encode($status, JSON_UNESCAPED_UNICODE | JSON_PRETTY_PRINT)
        );
    }

    private function patchJobProgress(string $jobId, int $progress, string $message): void
    {
        $this->assertNotCancelled($jobId);

        $current = $this->readJobStatus($jobId) ?? [];
        $current['progress'] = $progress;
        $current['message'] = $message;
        $current['status'] = $current['status'] ?? 'running';
        $this->writeJobStatus($jobId, $current);
    }

    private function jobFilePath(string $jobId): string
    {
        return self::JOB_DIR.DIRECTORY_SEPARATOR.$jobId.'.json';
    }

    private function dispatchDetached(string $jobId, string $filename): bool
    {
        try {
            $php = PHP_BINARY ?: 'php';
            $artisan = base_path('artisan');
            $logFile = storage_path('logs/gdrive-upload-'.$jobId.'.log');

            if (! function_exists('exec') && ! function_exists('proc_open')) {
                return false;
            }

            if (DIRECTORY_SEPARATOR === '\\') {
                $cmd = sprintf(
                    'start /B "" %s %s backup:upload-drive %s %s > %s 2>&1',
                    escapeshellarg($php),
                    escapeshellarg($artisan),
                    escapeshellarg($jobId),
                    escapeshellarg($filename),
                    escapeshellarg($logFile)
                );
                pclose(popen($cmd, 'r'));

                return true;
            }

            $cmd = sprintf(
                'nohup %s %s backup:upload-drive %s %s > %s 2>&1 &',
                escapeshellarg($php),
                escapeshellarg($artisan),
                escapeshellarg($jobId),
                escapeshellarg($filename),
                escapeshellarg($logFile)
            );
            exec($cmd);

            return true;
        } catch (\Throwable $e) {
            Log::warning('Detached Google Drive upload dispatch failed', [
                'message' => $e->getMessage(),
            ]);

            return false;
        }
    }

    public function frontendReturnUrl(string $status, ?string $message = null): string
    {
        $query = http_build_query(array_filter([
            'google_drive' => $status,
            'google_drive_message' => $message,
        ]));

        return FrontendUrlService::to('/settings'.($query !== '' ? '?'.$query : ''));
    }
}
