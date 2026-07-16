<?php

namespace App\Console\Commands;

use App\Services\GoogleDriveBackupService;
use Illuminate\Console\Command;

class RunGoogleDriveUploadCommand extends Command
{
    protected $signature = 'backup:upload-drive
        {jobId : UUID status job upload}
        {filename : Nama file zip backup}';

    protected $description = 'Unggah arsip backup ke Google Drive (resumable, multi-GB)';

    public function handle(GoogleDriveBackupService $drive): int
    {
        $jobId = (string) $this->argument('jobId');
        $filename = (string) $this->argument('filename');

        $this->info("Starting Drive upload job {$jobId} → {$filename}");
        $drive->runUploadJob($jobId, $filename);
        $this->info('Drive upload job finished');

        return self::SUCCESS;
    }
}
