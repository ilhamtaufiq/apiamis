<?php

namespace App\Services;

use App\Exceptions\BackupJobCancelledException;
use App\Models\AppSetting;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\File;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Storage;
use Illuminate\Support\Str;
use Spatie\MediaLibrary\MediaCollections\Models\Media;
use ZipArchive;
use ZipStream\ZipStream;
use ZipStream\OperationMode;

class SystemBackupService
{
    public const BACKUP_DIR = 'system-backups';

    private const SQL_MARKER = "/*__ARUMANIS_STMT__*/\n";

    /**
     * Build an S3 disk from AppSetting values (dynamic config).
     */
    public function getS3Disk(): \Illuminate\Contracts\Filesystem\Filesystem
    {
        $endpoint = AppSetting::getValue('s3_endpoint');
        $region = AppSetting::getValue('s3_region');
        $bucket = AppSetting::getValue('s3_bucket');
        $accessKeyId = AppSetting::getValue('s3_access_key_id');
        $secretAccessKey = AppSetting::getValue('s3_secret_access_key');

        if (!$region || !$bucket || !$accessKeyId || !$secretAccessKey) {
            throw new \RuntimeException('Pengaturan AWS S3 belum lengkap atau belum dikonfigurasi.');
        }

        return Storage::build([
            'driver' => 's3',
            'key' => $accessKeyId,
            'secret' => $secretAccessKey,
            'region' => $region,
            'bucket' => $bucket,
            'endpoint' => $endpoint ?: null,
            'use_path_style_endpoint' => (bool) $endpoint,
        ]);
    }

    public function isS3BackupEnabled(): bool
    {
        return AppSetting::getValue('s3_backup_enabled') === '1';
    }

    public function listBackups(): array
    {
        $backupDir = $this->ensureBackupDirectory();

        $local = collect(Storage::disk('local')->files($backupDir))
            ->filter(fn ($file) => Str::endsWith($file, '.zip'))
            ->map(function (string $file) {
                $absolutePath = Storage::disk('local')->path($file);

                return [
                    'filename' => basename($file),
                    'size' => File::exists($absolutePath) ? File::size($absolutePath) : 0,
                    'last_modified' => File::exists($absolutePath) ? File::lastModified($absolutePath) : null,
                    'storage' => 'local',
                ];
            });

        $s3 = collect();
        if ($this->isS3BackupEnabled()) {
            try {
                $s3Disk = $this->getS3Disk();
                $s3 = collect($s3Disk->files($backupDir))
                    ->filter(fn ($file) => Str::endsWith($file, '.zip'))
                    ->map(function (string $file) use ($s3Disk) {
                        return [
                            'filename' => basename($file),
                            'size' => $s3Disk->size($file),
                            'last_modified' => $s3Disk->lastModified($file),
                            'storage' => 's3',
                        ];
                    });
            } catch (\Throwable $e) {
                Log::warning('Gagal mengambil daftar backup dari S3', ['error' => $e->getMessage()]);
            }
        }

        return $local->merge($s3)
            ->sortByDesc('last_modified')
            ->values()
            ->map(function (array $item) {
                $item['download_url'] = url('/api/app-settings/backups/'.$item['filename']);

                return $item;
            })
            ->all();
    }

    public function queueBackup(?string $label, bool $includeMedia, bool $s3Direct = false): array
    {
        $jobId = (string) Str::uuid();
        $fileName = $this->buildBackupFilename($label);

        $this->writeJobStatus($jobId, [
            'job_id' => $jobId,
            'status' => 'queued',
            'filename' => $fileName,
            'include_media' => $includeMedia,
            's3_direct' => $s3Direct,
            'created_at' => now()->toIso8601String(),
            'message' => 'Backup masuk antrean',
            'progress' => 0,
        ]);

        $detached = $this->dispatchDetached($jobId, $fileName, $includeMedia, $s3Direct);

        if (! $detached) {
            // Fallback when exec/proc_open is disabled — runs after HTTP response.
            app()->terminating(function () use ($jobId, $fileName, $includeMedia, $s3Direct) {
                $this->runBackupJob($jobId, $fileName, $includeMedia, $s3Direct);
            });
        }

        return $this->readJobStatus($jobId) ?? [
            'job_id' => $jobId,
            'status' => 'queued',
            'filename' => $fileName,
            'include_media' => $includeMedia,
            's3_direct' => $s3Direct,
            'message' => 'Backup masuk antrean',
        ];
    }

    public function runBackupJob(string $jobId, string $fileName, bool $includeMedia, bool $s3Direct = false): void
    {
        ignore_user_abort(true);
        @set_time_limit(0);
        // Large media zips need headroom beyond default 128M/256M.
        @ini_set('memory_limit', '1024M');

        $this->recordWorkerPid($jobId, getmypid());

        $initialStatus = $this->readJobStatus($jobId) ?? [];
        $zipPath = Storage::disk('local')->path($this->backupFilePath($fileName));

        // If s3Direct requested but S3 not configured, fall back to local.
        $s3Direct = $s3Direct && $this->isS3BackupEnabled();

        if ($this->isCancelRequested($jobId)) {
            $this->finalizeCancelledJob($jobId, $fileName, $zipPath, $includeMedia, $initialStatus);

            return;
        }

        $this->writeJobStatus($jobId, [
            'job_id' => $jobId,
            'status' => 'running',
            'filename' => $fileName,
            'include_media' => $includeMedia,
            's3_direct' => $s3Direct,
            'created_at' => $initialStatus['created_at'] ?? now()->toIso8601String(),
            'started_at' => now()->toIso8601String(),
            'message' => 'Menyiapkan dump database',
            'progress' => 5,
        ]);

        try {
            $result = $this->createBackupArchive($jobId, $fileName, $includeMedia, $s3Direct);
            $runningStatus = $this->readJobStatus($jobId) ?? [];

            $this->writeJobStatus($jobId, [
                'job_id' => $jobId,
                'status' => 'completed',
                'filename' => $fileName,
                'include_media' => $includeMedia,
                'created_at' => $runningStatus['created_at'] ?? now()->toIso8601String(),
                'started_at' => $runningStatus['started_at'] ?? null,
                'finished_at' => now()->toIso8601String(),
                'message' => 'Backup berhasil dibuat',
                'progress' => 100,
                'result' => $result,
            ]);
        } catch (BackupJobCancelledException $exception) {
            $runningStatus = $this->readJobStatus($jobId) ?? [];
            $this->finalizeCancelledJob($jobId, $fileName, $zipPath, $includeMedia, $runningStatus);
        } catch (\Throwable $exception) {
            if ($this->isCancelRequested($jobId)) {
                $runningStatus = $this->readJobStatus($jobId) ?? [];
                $this->finalizeCancelledJob($jobId, $fileName, $zipPath, $includeMedia, $runningStatus);

                return;
            }

            Log::error('Backup job failed', [
                'job_id' => $jobId,
                'filename' => $fileName,
                'exception' => $exception,
            ]);

            if (File::exists($zipPath)) {
                @unlink($zipPath);
            }

            $runningStatus = $this->readJobStatus($jobId) ?? [];

            $this->writeJobStatus($jobId, [
                'job_id' => $jobId,
                'status' => 'failed',
                'filename' => $fileName,
                'include_media' => $includeMedia,
                'created_at' => $runningStatus['created_at'] ?? now()->toIso8601String(),
                'started_at' => $runningStatus['started_at'] ?? null,
                'finished_at' => now()->toIso8601String(),
                'message' => 'Backup gagal dibuat',
                'progress' => 0,
                'error' => $exception->getMessage(),
            ]);
        }
    }

    public function readJobStatus(string $jobId): ?array
    {
        $this->guardJobId($jobId);
        $path = $this->jobFilePath($jobId);
        if (! Storage::disk('local')->exists($path)) {
            return null;
        }

        $status = json_decode(Storage::disk('local')->get($path), true);

        return is_array($status) ? $status : null;
    }

    public function cancelJob(string $jobId): array
    {
        $status = $this->readJobStatus($jobId);
        abort_unless($status !== null, 404, 'Status backup tidak ditemukan');

        $state = (string) ($status['status'] ?? '');
        if (in_array($state, ['completed', 'failed', 'cancelled'], true)) {
            return $status;
        }

        $this->writeJobStatus($jobId, array_merge($status, [
            'cancel_requested' => true,
            'cancel_requested_at' => now()->toIso8601String(),
            'message' => 'Membatalkan backup…',
        ]));

        self::terminateProcessPid(isset($status['pid']) ? (int) $status['pid'] : null);

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

    public static function terminateProcessPid(?int $pid): void
    {
        if ($pid === null || $pid <= 0) {
            return;
        }

        if (DIRECTORY_SEPARATOR === '\\') {
            @exec('taskkill /PID '.(int) $pid.' /F');

            return;
        }

        @exec('kill -TERM '.(int) $pid.' 2>/dev/null');
        usleep(300_000);
        $stillRunning = shell_exec('kill -0 '.(int) $pid.' 2>/dev/null; echo $?');
        if (trim((string) $stillRunning) === '0') {
            @exec('kill -KILL '.(int) $pid.' 2>/dev/null');
        }
    }

    public function backupAbsolutePath(string $filename): string
    {
        $this->guardFilename($filename);

        return Storage::disk('local')->path($this->backupFilePath($filename));
    }

    public function deleteBackup(string $filename): void
    {
        $this->guardFilename($filename);
        $path = $this->backupFilePath($filename);

        // Delete from local storage if present
        if (Storage::disk('local')->exists($path)) {
            Storage::disk('local')->delete($path);
        }

        // Also delete from S3 if enabled
        if ($this->isS3BackupEnabled()) {
            try {
                $s3Disk = $this->getS3Disk();
                $s3Path = "{$this->BACKUP_DIR}/{$filename}";
                $s3Disk->delete($s3Path);
            } catch (\Throwable $e) {
                Log::warning('Gagal menghapus backup dari S3', [
                    'filename' => $filename,
                    'error' => $e->getMessage(),
                ]);
            }
        }

        abort_unless(Storage::disk('local')->exists($path) || $this->isS3BackupEnabled(), 404, 'Backup tidak ditemukan');
    }

    /**
     * Get the filesystem (local or S3) where this backup lives.
     */
    public function getBackupDisk(string $filename): string
    {
        $path = $this->backupFilePath($filename);
        if (Storage::disk('local')->exists($path)) {
            return 'local';
        }
        if ($this->isS3BackupEnabled()) {
            try {
                $s3Disk = $this->getS3Disk();
                $s3Path = "{$this->BACKUP_DIR}/{$filename}";
                if ($s3Disk->exists($s3Path)) {
                    return 's3';
                }
            } catch (\Throwable $e) {
                Log::warning('Gagal memeriksa penyimpanan S3', [
                    'filename' => $filename,
                    'error' => $e->getMessage(),
                ]);
            }
        }
        return 'local';
    }

    public function restoreArchive(string $zipPath): array
    {
        $zipSize = File::exists($zipPath) ? File::size($zipPath) : 0;
        // Need room for extract (~zip size) plus media copy — require ~1.5× archive free.
        $freeBytes = @disk_free_space(storage_path('app'));
        if (is_int($freeBytes) && $zipSize > 0 && $freeBytes < (int) ($zipSize * 1.5)) {
            throw new \RuntimeException(
                'Ruang disk tidak cukup untuk restore. Butuh sekitar '.
                round(($zipSize * 1.5) / 1_073_741_824, 1).
                ' GB bebas; tersedia '.
                round($freeBytes / 1_073_741_824, 1).
                ' GB.'
            );
        }

        $extractDir = storage_path('app/tmp/restore_'.Str::uuid());
        File::ensureDirectoryExists($extractDir);

        $zip = new ZipArchive;
        if ($zip->open($zipPath) !== true) {
            throw new \RuntimeException('Gagal membuka file backup');
        }

        try {
            // Large archives: extract without loading whole zip into RAM as string.
            if (! $zip->extractTo($extractDir)) {
                throw new \RuntimeException('Gagal mengekstrak arsip backup');
            }
            $zip->close();

            $sqlPath = $extractDir.DIRECTORY_SEPARATOR.'database.sql';
            abort_unless(File::exists($sqlPath), 422, 'Database dump tidak ditemukan di backup');

            DB::statement('SET FOREIGN_KEY_CHECKS=0');
            $this->restoreSqlFromFile($sqlPath);
            $this->restoreMediaFiles($extractDir);
            DB::statement('SET FOREIGN_KEY_CHECKS=1');

            return [
                'restored_at' => now()->toIso8601String(),
                'source' => basename($zipPath),
            ];
        } finally {
            DB::statement('SET FOREIGN_KEY_CHECKS=1');
            File::deleteDirectory($extractDir);
        }
    }

    public function guardFilename(string $filename): void
    {
        abort_unless(preg_match('/^[A-Za-z0-9._-]+\.zip$/', $filename) === 1, 422, 'Nama backup tidak valid');
    }

    public function guardJobId(string $jobId): void
    {
        abort_unless(preg_match('/^[A-Za-z0-9-]+$/', $jobId) === 1, 422, 'ID backup tidak valid');
    }

    private function dispatchDetached(string $jobId, string $fileName, bool $includeMedia, bool $s3Direct = false): bool
    {
        try {
            $php = PHP_BINARY ?: 'php';
            $artisan = base_path('artisan');
            $mediaFlag = $includeMedia ? '1' : '0';
            $s3Flag = $s3Direct ? '1' : '0';
            $logFile = storage_path('logs/backup-'.$jobId.'.log');

            if (! function_exists('exec') && ! function_exists('proc_open')) {
                return false;
            }

            if (DIRECTORY_SEPARATOR === '\\') {
                // Windows (Laragon): start detached without waiting.
                $cmd = sprintf(
                    'start /B "" %s %s backup:run %s %s %s %s > %s 2>&1',
                    escapeshellarg($php),
                    escapeshellarg($artisan),
                    escapeshellarg($jobId),
                    escapeshellarg($fileName),
                    escapeshellarg($mediaFlag),
                    escapeshellarg($s3Flag),
                    escapeshellarg($logFile)
                );
                if (function_exists('popen')) {
                    pclose(popen($cmd, 'r'));

                    return true;
                }

                return false;
            }

            $cmd = sprintf(
                'nohup %s %s backup:run %s %s %s %s > %s 2>&1 &',
                escapeshellarg($php),
                escapeshellarg($artisan),
                escapeshellarg($jobId),
                escapeshellarg($fileName),
                escapeshellarg($mediaFlag),
                escapeshellarg($s3Flag),
                escapeshellarg($logFile)
            );
            exec($cmd);

            return true;
        } catch (\Throwable $e) {
            Log::warning('Detached backup dispatch failed, will fall back', [
                'error' => $e->getMessage(),
            ]);

            return false;
        }
    }

    private function createBackupArchive(string $jobId, string $fileName, bool $includeMedia, bool $s3Direct = false): array
    {
        $sqlTempPath = tempnam(sys_get_temp_dir(), 'arumanis_sql_');
        if ($sqlTempPath === false) {
            throw new \RuntimeException('Gagal menyiapkan file backup');
        }

        if ($s3Direct && $this->isS3BackupEnabled()) {
            return $this->createBackupArchiveS3($jobId, $fileName, $includeMedia, $sqlTempPath);
        }

        return $this->createBackupArchiveLocal($jobId, $fileName, $includeMedia, $sqlTempPath);
    }

    /**
     * Stream backup ZIP directly to S3 — no intermediate local archive.
     */
    private function createBackupArchiveS3(string $jobId, string $fileName, bool $includeMedia, string $sqlTempPath): array
    {
        try {
            $this->patchJob($jobId, [
                'message' => 'Membuat dump database…',
                'progress' => 10,
            ]);
            $this->dumpDatabase($sqlTempPath, $jobId);

            $this->patchJob($jobId, [
                'message' => 'Streaming arsip ZIP langsung ke S3…',
                'progress' => 30,
            ]);

            $s3Disk = $this->getS3Disk();
            $s3Client = $s3Disk->getClient();
            $s3Client->registerStreamWrapper();

            $bucket = AppSetting::getValue('s3_bucket');
            $s3Path = "s3://{$bucket}/system-backups/{$fileName}";

            $writeStream = fopen($s3Path, 'wb');
            if ($writeStream === false) {
                throw new \RuntimeException('Gagal membuka stream ke S3 bucket');
            }

            $zip = new ZipStream(
                operationMode: OperationMode::NORMAL,
                outputStream: $writeStream,
                sendHttpHeaders: false,
            );

            $dbStream = fopen($sqlTempPath, 'rb');
            $zip->addFileFromStream('database.sql', $dbStream);
            fclose($dbStream);

            $mediaCount = 0;
            if ($includeMedia) {
                $this->patchJob($jobId, [
                    'message' => 'Streaming file media ke S3…',
                    'progress' => 40,
                ]);
                $mediaCount = $this->addMediaFilesToZipStream($zip, function (int $done, int $total) use ($jobId) {
                    $pct = $total > 0 ? 40 + (int) floor(($done / $total) * 50) : 70;
                    $this->patchJob($jobId, [
                        'message' => "Streaming media {$done}/{$total}",
                        'progress' => min(90, $pct),
                    ]);
                });
            }

            $this->patchJob($jobId, [
                'message' => 'Menutup arsip ZIP di S3…',
                'progress' => 95,
            ]);

            $zip->finish();
            fclose($writeStream);

            $size = $s3Disk->size('system-backups/'.$fileName);

            return [
                'filename' => $fileName,
                'download_url' => url('/api/app-settings/backups/'.$fileName),
                'size' => $size,
                'include_media' => $includeMedia,
                'media_files' => $mediaCount,
                'storage' => 's3',
            ];
        } catch (\Throwable $e) {
            throw new \RuntimeException('Backup ke S3 gagal: '.$e->getMessage());
        } finally {
            @unlink($sqlTempPath);
        }
    }

    /**
     * Local backup (existing behavior when S3 is not enabled).
     */
    private function createBackupArchiveLocal(string $jobId, string $fileName, bool $includeMedia, string $sqlTempPath): array
    {
        $backupDir = $this->ensureBackupDirectory();
        $zipPath = Storage::disk('local')->path($backupDir.DIRECTORY_SEPARATOR.$fileName);

        // Free disk space check (need roughly archive size headroom; unknown up front — require 1GB free).
        $freeBytes = @disk_free_space(dirname($zipPath));
        if (is_int($freeBytes) && $freeBytes < 1_073_741_824) {
            throw new \RuntimeException('Ruang disk server kurang dari 1 GB. Bebaskan ruang sebelum backup besar.');
        }

        try {
            $this->patchJob($jobId, [
                'message' => 'Membuat dump database…',
                'progress' => 10,
            ]);
            $this->dumpDatabase($sqlTempPath, $jobId);

            $this->patchJob($jobId, [
                'message' => 'Membuka arsip ZIP…',
                'progress' => 30,
            ]);

            $zip = new ZipArchive;
            $result = $zip->open($zipPath, ZipArchive::CREATE | ZipArchive::OVERWRITE);
            if ($result !== true) {
                throw new \RuntimeException('Gagal membuka arsip backup (kode '.$result.')');
            }

            $zip->addFile($sqlTempPath, 'database.sql');
            $zip->setCompressionName('database.sql', ZipArchive::CM_DEFLATE, 6);

            $mediaCount = 0;
            if ($includeMedia) {
                $this->patchJob($jobId, [
                    'message' => 'Mengemas file media (bisa memakan waktu untuk arsip multi-GB)…',
                    'progress' => 40,
                ]);
                $mediaCount = $this->addMediaFilesToZip($zip, function (int $done, int $total) use ($jobId) {
                    $pct = $total > 0 ? 40 + (int) floor(($done / $total) * 50) : 70;
                    $this->patchJob($jobId, [
                        'message' => "Mengemas media {$done}/{$total}",
                        'progress' => min(90, $pct),
                    ]);
                });
            }

            $this->patchJob($jobId, [
                'message' => 'Menutup arsip ZIP…',
                'progress' => 95,
            ]);

            $zip->setArchiveComment(json_encode([
                'created_at' => now()->toIso8601String(),
                'database' => DB::connection()->getDatabaseName(),
                'include_media' => $includeMedia,
                'media_files' => $mediaCount,
            ]));

            if (! $zip->close()) {
                throw new \RuntimeException('Gagal menutup arsip backup');
            }

            $size = File::exists($zipPath) ? File::size($zipPath) : 0;
            if ($size <= 0) {
                throw new \RuntimeException('Arsip backup kosong atau gagal ditulis');
            }

            return [
                'filename' => $fileName,
                'download_url' => url('/api/app-settings/backups/'.$fileName),
                'size' => $size,
                'include_media' => $includeMedia,
                'media_files' => $mediaCount,
            ];
        } catch (\Throwable $e) {
            if (File::exists($zipPath)) {
                @unlink($zipPath);
            }
            throw $e;
        } finally {
            @unlink($sqlTempPath);
        }
    }

    private function patchJob(string $jobId, array $patch): void
    {
        $this->assertNotCancelled($jobId);

        $current = $this->readJobStatus($jobId) ?? [];
        $this->writeJobStatus($jobId, array_merge($current, $patch, [
            'job_id' => $jobId,
            'status' => $current['status'] ?? 'running',
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
            throw new BackupJobCancelledException();
        }
    }

    private function finalizeCancelledJob(
        string $jobId,
        string $fileName,
        string $zipPath,
        bool $includeMedia,
        array $previousStatus,
    ): void {
        if (File::exists($zipPath)) {
            @unlink($zipPath);
        }

        $this->writeJobStatus($jobId, [
            'job_id' => $jobId,
            'status' => 'cancelled',
            'filename' => $fileName,
            'include_media' => $includeMedia,
            'created_at' => $previousStatus['created_at'] ?? now()->toIso8601String(),
            'started_at' => $previousStatus['started_at'] ?? null,
            'finished_at' => now()->toIso8601String(),
            'message' => 'Backup dibatalkan',
            'progress' => 0,
            'cancel_requested' => true,
        ]);
    }

    private function dumpDatabase(string $targetPath, ?string $jobId = null): void
    {
        $pdo = DB::connection()->getPdo();
        $schema = DB::connection()->getSchemaBuilder();
        $databaseName = DB::connection()->getDatabaseName();

        $tables = DB::select("SHOW FULL TABLES WHERE Table_type = 'BASE TABLE'");
        $tableNames = array_map(static function ($table) {
            return array_values((array) $table)[0];
        }, $tables);

        $handle = fopen($targetPath, 'wb');
        if ($handle === false) {
            throw new \RuntimeException('Gagal menulis file dump database');
        }

        try {
            fwrite($handle, "-- Arumanis backup\n");
            fwrite($handle, "-- Database: {$databaseName}\n");
            fwrite($handle, "SET FOREIGN_KEY_CHECKS=0;\n");
            fwrite($handle, "SET SQL_MODE='NO_AUTO_VALUE_ON_ZERO';\n");
            fwrite($handle, self::SQL_MARKER);

            foreach ($tableNames as $tableName) {
                if ($jobId !== null) {
                    $this->assertNotCancelled($jobId);
                }

                $escapedTable = str_replace('`', '``', $tableName);
                $create = DB::selectOne("SHOW CREATE TABLE `{$escapedTable}`");
                $createSql = (string) ($create->{'Create Table'} ?? array_values((array) $create)[1] ?? '');

                fwrite($handle, "DROP TABLE IF EXISTS `{$escapedTable}`;\n".self::SQL_MARKER);
                fwrite($handle, $createSql.";\n".self::SQL_MARKER);

                $columns = $schema->getColumnListing($tableName);
                $rows = DB::table($tableName)->cursor();
                $batch = [];

                foreach ($rows as $row) {
                    $values = [];
                    foreach ($columns as $column) {
                        $values[] = $this->quoteValue($pdo, data_get($row, $column));
                    }

                    $batch[] = '('.implode(', ', $values).')';

                    if (count($batch) >= 100) {
                        fwrite($handle, $this->buildInsertStatement($escapedTable, $columns, $batch).self::SQL_MARKER);
                        $batch = [];
                    }
                }

                if ($batch !== []) {
                    fwrite($handle, $this->buildInsertStatement($escapedTable, $columns, $batch).self::SQL_MARKER);
                }
            }

            fwrite($handle, "SET FOREIGN_KEY_CHECKS=1;\n");
        } finally {
            fclose($handle);
        }
    }

    private function buildInsertStatement(string $table, array $columns, array $rows): string
    {
        $columnList = implode(', ', array_map(fn ($column) => '`'.str_replace('`', '``', $column).'`', $columns));

        return "INSERT INTO `{$table}` ({$columnList}) VALUES\n".implode(",\n", $rows).";\n";
    }

    private function quoteValue(\PDO $pdo, mixed $value): string
    {
        if ($value === null) {
            return 'NULL';
        }

        if (is_bool($value)) {
            return $value ? '1' : '0';
        }

        if (is_int($value) || is_float($value)) {
            return (string) $value;
        }

        if (is_array($value) || is_object($value)) {
            $value = json_encode($value, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
        }

        return $pdo->quote((string) $value);
    }

    /**
     * @param  callable(int $done, int $total):void|null  $onProgress
     */
    private function addMediaFilesToZip(ZipArchive $zip, ?callable $onProgress = null): int
    {
        $mediaItems = Media::query()->get(['id', 'disk', 'file_name', 'collection_name', 'model_type', 'model_id']);
        $directories = [];

        foreach ($mediaItems as $media) {
            $disk = $media->disk ?: config('filesystems.default', 'public');
            $relativePath = $media->getPathRelativeToRoot();
            $directory = trim(str_replace(['\\', '/'], DIRECTORY_SEPARATOR, dirname($relativePath)), DIRECTORY_SEPARATOR);
            $directories[$disk][$directory === '.' ? '' : $directory] = true;
        }

        // Pre-count files for progress (extra scan; acceptable for UX on multi-GB jobs).
        $allFiles = [];
        foreach ($directories as $disk => $paths) {
            foreach (array_keys($paths) as $path) {
                foreach (Storage::disk($disk)->allFiles($path) as $file) {
                    $absolutePath = Storage::disk($disk)->path($file);
                    if (! File::exists($absolutePath)) {
                        continue;
                    }
                    $allFiles[] = [$disk, $file, $absolutePath];
                }
            }
        }

        $total = count($allFiles);
        $count = 0;
        $lastReport = 0;

        foreach ($allFiles as [$disk, $file, $absolutePath]) {
            $archiveName = 'media/'.$disk.'/'.ltrim(str_replace(['\\', '/'], '/', $file), '/');
            if (! $zip->addFile($absolutePath, $archiveName)) {
                Log::warning('Backup: gagal menambahkan file media', ['file' => $file]);
                continue;
            }
            // STORE = no recompress (photos/PDFs already compressed) — faster for multi-GB.
            $zip->setCompressionName($archiveName, ZipArchive::CM_STORE);
            $count++;

            if ($onProgress && ($count - $lastReport >= 25 || $count === $total)) {
                $onProgress($count, $total);
                $lastReport = $count;
            }
        }

        return $count;
    }

    /**
     * Add media files directly to a ZipStream output (for S3 streaming backup).
     */
    private function addMediaFilesToZipStream(ZipStream $zip, ?callable $onProgress = null): int
    {
        $mediaItems = Media::query()->get(['id', 'disk', 'file_name', 'collection_name', 'model_type', 'model_id']);
        $directories = [];

        foreach ($mediaItems as $media) {
            $disk = $media->disk ?: config('filesystems.default', 'public');
            $relativePath = $media->getPathRelativeToRoot();
            $directory = trim(str_replace(['\\', '/'], DIRECTORY_SEPARATOR, dirname($relativePath)), DIRECTORY_SEPARATOR);
            $directories[$disk][$directory === '.' ? '' : $directory] = true;
        }

        $allFiles = [];
        foreach ($directories as $disk => $paths) {
            foreach (array_keys($paths) as $path) {
                foreach (Storage::disk($disk)->allFiles($path) as $file) {
                    $absolutePath = Storage::disk($disk)->path($file);
                    if (! File::exists($absolutePath)) {
                        continue;
                    }
                    $allFiles[] = [$disk, $file, $absolutePath];
                }
            }
        }

        $total = count($allFiles);
        $count = 0;
        $lastReport = 0;

        foreach ($allFiles as [$disk, $file, $absolutePath]) {
            $archiveName = 'media/'.$disk.'/'.ltrim(str_replace(['\\', '/'], '/', $file), '/');
            $stream = fopen($absolutePath, 'rb');
            if ($stream === false) {
                Log::warning('Backup S3: gagal membuka file media', ['file' => $file]);
                continue;
            }
            $zip->addFileFromStream($archiveName, $stream);
            fclose($stream);
            $count++;

            if ($onProgress && ($count - $lastReport >= 25 || $count === $total)) {
                $onProgress($count, $total);
                $lastReport = $count;
            }
        }

        return $count;
    }

    private function restoreSqlFromFile(string $sqlPath): void
    {
        $handle = fopen($sqlPath, 'rb');
        if ($handle === false) {
            throw new \RuntimeException('Gagal membaca dump database');
        }

        try {
            $buffer = '';
            $marker = self::SQL_MARKER;

            while (! feof($handle)) {
                $chunk = fread($handle, 1024 * 1024);
                if ($chunk === false) {
                    break;
                }
                $buffer .= $chunk;

                while (($pos = strpos($buffer, $marker)) !== false) {
                    $statement = trim(substr($buffer, 0, $pos));
                    $buffer = substr($buffer, $pos + strlen($marker));

                    if ($statement === '' || str_starts_with($statement, '--')) {
                        continue;
                    }

                    DB::unprepared(rtrim($statement, ";\r\n\t "));
                }
            }

            $tail = trim($buffer);
            if ($tail !== '' && ! str_starts_with($tail, '--')) {
                DB::unprepared(rtrim($tail, ";\r\n\t "));
            }
        } finally {
            fclose($handle);
        }
    }

    private function restoreMediaFiles(string $extractDir): void
    {
        $mediaRoot = $extractDir.DIRECTORY_SEPARATOR.'media';
        if (! File::exists($mediaRoot)) {
            return;
        }

        $files = File::allFiles($mediaRoot);
        foreach ($files as $file) {
            $relative = Str::after($file->getPathname(), $mediaRoot.DIRECTORY_SEPARATOR);
            $relative = str_replace(['\\', '/'], DIRECTORY_SEPARATOR, $relative);
            $parts = explode(DIRECTORY_SEPARATOR, $relative, 2);

            if (count($parts) < 2) {
                continue;
            }

            [$disk, $path] = $parts;
            $targetPath = Storage::disk($disk)->path($path);
            File::ensureDirectoryExists(dirname($targetPath));
            File::copy($file->getPathname(), $targetPath);
        }
    }

    private function writeJobStatus(string $jobId, array $status): void
    {
        $this->guardJobId($jobId);
        Storage::disk('local')->put(
            $this->jobFilePath($jobId),
            json_encode($status, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES)
        );
    }

    private function ensureBackupDirectory(): string
    {
        $dir = self::BACKUP_DIR;
        if (! Storage::disk('local')->exists($dir)) {
            Storage::disk('local')->makeDirectory($dir);
        }

        return $dir;
    }

    private function ensureJobDirectory(): string
    {
        $dir = self::BACKUP_DIR.DIRECTORY_SEPARATOR.'jobs';
        if (! Storage::disk('local')->exists($dir)) {
            Storage::disk('local')->makeDirectory($dir);
        }

        return $dir;
    }

    private function jobFilePath(string $jobId): string
    {
        return $this->ensureJobDirectory().DIRECTORY_SEPARATOR.$jobId.'.json';
    }

    private function backupFilePath(string $filename): string
    {
        return self::BACKUP_DIR.DIRECTORY_SEPARATOR.$filename;
    }

    private function buildBackupFilename(?string $label = null): string
    {
        $parts = ['arumanis', now()->format('Ymd_His')];

        if ($label !== null && trim($label) !== '') {
            $parts[] = Str::slug($label);
        }

        return implode('_', array_filter($parts)).'.zip';
    }
}
