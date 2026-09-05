<?php

namespace App\Http\Controllers;

use App\Services\SystemBackupService;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\File;
use Illuminate\Support\Facades\Storage;
use Illuminate\Support\Str;

class BackupController extends Controller
{
    public function __construct(
        private readonly SystemBackupService $backups,
    ) {}

    public function index()
    {
        return response()->json([
            'data' => $this->backups->listBackups(),
        ]);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'label' => 'nullable|string|max:80',
            'include_media' => 'nullable|boolean',
            's3_direct' => 'nullable|boolean',
        ]);

        $includeMedia = $request->boolean('include_media', true);
        $s3Direct = $request->boolean('s3_direct', false);
        $status = $this->backups->queueBackup($validated['label'] ?? null, $includeMedia, $s3Direct);

        return response()->json([
            'data' => $status,
            'message' => $s3Direct
                ? 'Backup sedang diproses langsung ke S3'
                : 'Backup sedang diproses di server',
        ], 202);
    }

    public function showJob(string $jobId)
    {
        $this->backups->guardJobId($jobId);
        $status = $this->backups->readJobStatus($jobId);

        abort_unless($status !== null, 404, 'Status backup tidak ditemukan');

        return response()->json([
            'data' => $status,
        ]);
    }

    public function cancelJob(string $jobId)
    {
        $status = $this->backups->cancelJob($jobId);

        return response()->json([
            'data' => $status,
            'message' => $status['status'] === 'cancelled'
                ? 'Backup dibatalkan'
                : 'Permintaan pembatalan backup dikirim',
        ]);
    }

    public function download(string $filename)
    {
        $this->backups->guardFilename($filename);
        $disk = $this->backups->getBackupDisk($filename);

        if ($disk === 's3') {
            $s3Disk = $this->backups->getS3Disk();
            $s3Path = "system-backups/{$filename}";
            abort_unless($s3Disk->exists($s3Path), 404, 'Backup tidak ditemukan di S3');

            return response()->streamDownload(function () use ($s3Disk, $s3Path) {
                $s3Disk->download($s3Path);
            }, $filename, [
                'Content-Type' => 'application/zip',
                'X-Accel-Buffering' => 'no',
                'Cache-Control' => 'no-store, private',
            ]);
        }

        $path = $this->backups->backupAbsolutePath($filename);
        abort_unless(File::exists($path), 404, 'Backup tidak ditemukan');

        // BinaryFileResponse streams from disk (supports multi-GB). Avoid buffering
        // in nginx / reverse proxies in front of the API.
        return response()->download($path, $filename, [
            'Content-Type' => 'application/zip',
            'X-Accel-Buffering' => 'no',
            'Cache-Control' => 'no-store, private',
        ]);
    }

    public function destroy(string $filename)
    {
        $this->backups->deleteBackup($filename);

        return response()->json([
            'message' => 'Backup berhasil dihapus',
        ]);
    }

    public function restore(Request $request)
    {
        $validated = $request->validate([
            'backup_name' => 'nullable|string',
            'backup_file' => 'nullable|file|mimes:zip|max:51200', // 50MB — matches PHP upload limit
        ]);

        if (! $request->hasFile('backup_file') && empty($validated['backup_name'])) {
            return response()->json([
                'message' => 'Pilih file backup atau nama backup yang tersimpan',
            ], 422);
        }

        if ($request->hasFile('backup_file')) {
            $upload = $request->file('backup_file');
            // Multi-GB restore must use server-side archive (list backup), not browser upload.
            if ($upload->getSize() > 50 * 1024 * 1024) {
                return response()->json([
                    'message' => 'Upload restore dibatasi 50 MB. Untuk backup besar, unggah lewat server atau pilih backup yang sudah tersimpan di daftar.',
                ], 422);
            }
        }

        $sourcePath = null;
        $tempZipPath = null;

        try {
            if ($request->hasFile('backup_file')) {
                $tempZipPath = $request->file('backup_file')->storeAs(
                    'tmp',
                    'restore_'.Str::uuid().'.zip',
                    'local'
                );
                $sourcePath = Storage::disk('local')->path($tempZipPath);
            } else {
                $this->backups->guardFilename($validated['backup_name']);
                $filename = $validated['backup_name'];
                $sourcePath = $this->backups->backupAbsolutePath($filename);

                if (! File::exists($sourcePath)) {
                    // Backup may live only on S3 (s3_direct backups never land locally).
                    $disk = $this->backups->getBackupDisk($filename);
                    if ($disk !== 's3') {
                        abort(404, 'Backup tidak ditemukan');
                    }

                    // Pull a local copy first — ZipArchive cannot read s3:// streams.
                    $s3Disk = $this->backups->getS3Disk();
                    $s3Path = "system-backups/{$filename}";
                    abort_unless($s3Disk->exists($s3Path), 404, 'Backup tidak ditemukan');

                    @set_time_limit(0);
                    $localStream = fopen($sourcePath, 'w+b');
                    $remoteStream = $s3Disk->readStream($s3Path);
                    if ($localStream === false || $remoteStream === false) {
                        if (is_resource($localStream)) {
                            fclose($localStream);
                        }
                        if (is_resource($remoteStream)) {
                            fclose($remoteStream);
                        }
                        abort(502, 'Gagal mengunduh backup dari S3');
                    }
                    stream_copy_to_stream($remoteStream, $localStream);
                    fclose($remoteStream);
                    fclose($localStream);
                    $tempZipPath = 'system-backups/'.$filename;
                }
            }

            @set_time_limit(0);
            @ini_set('memory_limit', '1024M');

            $result = $this->backups->restoreArchive($sourcePath);

            return response()->json([
                'data' => $result,
                'message' => 'Restore backup berhasil dijalankan',
            ]);
        } finally {
            if ($tempZipPath) {
                Storage::disk('local')->delete($tempZipPath);
            }
        }
    }

    public function testS3Connection(Request $request)
    {
        $request->validate([
            's3_endpoint' => 'nullable|string|max:255',
            's3_region' => 'nullable|string|max:64',
            's3_bucket' => 'nullable|string|max:64',
            's3_access_key_id' => 'nullable|string|max:128',
            's3_secret_access_key' => 'nullable|string|max:255',
        ]);

        $endpoint = $request->input('s3_endpoint') ?: \App\Models\AppSetting::getValue('s3_endpoint');
        $region = $request->input('s3_region') ?: \App\Models\AppSetting::getValue('s3_region');
        $bucket = $request->input('s3_bucket') ?: \App\Models\AppSetting::getValue('s3_bucket');
        $accessKeyId = $request->input('s3_access_key_id') ?: \App\Models\AppSetting::getValue('s3_access_key_id');
        $secretAccessKey = $request->input('s3_secret_access_key') ?: \App\Models\AppSetting::getValue('s3_secret_access_key');

        $usedStoredKey = !$request->filled('s3_secret_access_key');

        if (!$region || !$bucket || !$accessKeyId || !$secretAccessKey) {
            return response()->json([
                'ok' => false,
                'error' => 'Region, Bucket, Access Key, dan Secret Key wajib diisi untuk uji koneksi.',
                'used_stored_key' => $usedStoredKey,
            ], 400);
        }

        try {
            $disk = Storage::build([
                'driver' => 's3',
                'key' => $accessKeyId,
                'secret' => $secretAccessKey,
                'region' => $region,
                'bucket' => $bucket,
                'endpoint' => $endpoint ?: null,
                'use_path_style_endpoint' => (bool) $endpoint,
            ]);

            $disk->files('', false);

            return response()->json([
                'ok' => true,
                'used_stored_key' => $usedStoredKey,
            ]);
        } catch (\Throwable $e) {
            return response()->json([
                'ok' => false,
                'error' => 'Koneksi S3 gagal: ' . $e->getMessage(),
                'used_stored_key' => $usedStoredKey,
            ], 422);
        }
    }
}
