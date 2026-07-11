<?php

namespace App\Console\Commands;

use App\Services\SystemBackupService;
use Illuminate\Console\Command;

class RunSystemBackupCommand extends Command
{
    protected $signature = 'backup:run
        {jobId : UUID status job}
        {filename : Nama file zip tujuan}
        {includeMedia=1 : 1 sertakan media, 0 database saja}';

    protected $description = 'Jalankan job backup sistem di proses CLI (untuk arsip multi-GB)';

    public function handle(SystemBackupService $backups): int
    {
        $jobId = (string) $this->argument('jobId');
        $filename = (string) $this->argument('filename');
        $includeMedia = ((string) $this->argument('includeMedia')) !== '0';

        $this->info("Starting backup job {$jobId} → {$filename}");
        $backups->runBackupJob($jobId, $filename, $includeMedia);
        $this->info('Backup job finished');

        return self::SUCCESS;
    }
}
