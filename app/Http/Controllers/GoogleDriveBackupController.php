<?php

namespace App\Http\Controllers;

use App\Services\GoogleDriveBackupService;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Log;

class GoogleDriveBackupController extends Controller
{
    public function __construct(
        private readonly GoogleDriveBackupService $drive,
    ) {}

    public function status()
    {
        return response()->json([
            'data' => $this->drive->status(),
        ]);
    }

    public function connect()
    {
        return response()->json([
            'data' => [
                'url' => $this->drive->buildConnectUrl(),
            ],
        ]);
    }

    public function callback(Request $request)
    {
        try {
            if ($request->filled('error')) {
                throw new \RuntimeException((string) $request->query('error'));
            }

            $code = (string) $request->query('code', '');
            $state = $request->query('state');
            abort_unless($code !== '', 422, 'Kode OAuth tidak ada');

            $this->drive->handleCallback($code, is_string($state) ? $state : null);

            return redirect()->away($this->drive->frontendReturnUrl('connected'));
        } catch (\Throwable $e) {
            Log::warning('Google Drive OAuth callback failed', [
                'message' => $e->getMessage(),
            ]);

            return redirect()->away(
                $this->drive->frontendReturnUrl('error', $e->getMessage())
            );
        }
    }

    public function disconnect()
    {
        $this->drive->disconnect();

        return response()->json([
            'message' => 'Koneksi Google Drive diputus',
            'data' => $this->drive->status(),
        ]);
    }

    public function upload(string $filename)
    {
        $status = $this->drive->queueUpload($filename);

        return response()->json([
            'data' => $status,
            'message' => 'Upload ke Google Drive sedang diproses',
        ], 202);
    }

    public function showUploadJob(string $jobId)
    {
        $status = $this->drive->readJobStatus($jobId);
        abort_unless($status !== null, 404, 'Status upload tidak ditemukan');

        return response()->json([
            'data' => $status,
        ]);
    }

    public function cancelUploadJob(string $jobId)
    {
        $status = $this->drive->cancelUploadJob($jobId);

        return response()->json([
            'data' => $status,
            'message' => $status['status'] === 'cancelled'
                ? 'Upload ke Google Drive dibatalkan'
                : 'Permintaan pembatalan upload dikirim',
        ]);
    }
}
