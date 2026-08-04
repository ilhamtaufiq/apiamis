<?php

namespace App\Console\Commands;

use App\Services\SystemBackupService;
use Illuminate\Console\Command;

class RunSystemBackupCommand extends Command
{
    protected $signature = 'backup:run
        {jobId : UUID status job}
        {filename : Nama file zip tujuan}
        {includeMedia=1 : 1 sertakan media, 0 database saja}
        {s3Direct=0 : 1 stream langsung ke S3, 0 simpan lokal}';

    protected $description = 'Jalankan job backup sistem di proses CLI (untuk arsip multi-GB)';

    public function handle(SystemBackupService $backups): int
    {
        $jobId = (string) $this->argument('jobId');
        $filename = (string) $this->argument('filename');
        $includeMedia = ((string) $this->argument('includeMedia')) !== '0';
        $s3Direct = ((string) $this->argument('s3Direct')) === '1';

        $this->info("Starting backup job {$jobId} → {$filename} (s3Direct=".($s3Direct ? 'yes' : 'no').")");
        $backups->runBackupJob($jobId, $filename, $includeMedia, $s3Direct);
        $this->info('Backup job finished');

        return self::SUCCESS;
    }
}
