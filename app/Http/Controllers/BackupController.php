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
        ]);

        $includeMedia = $request->boolean('include_media', true);
        $status = $this->backups->queueBackup($validated['label'] ?? null, $includeMedia);

        return response()->json([
            'data' => $status,
            'message' => 'Backup sedang diproses di server',
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
                $sourcePath = $this->backups->backupAbsolutePath($validated['backup_name']);
                abort_unless(File::exists($sourcePath), 404, 'Backup tidak ditemukan');
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
}
