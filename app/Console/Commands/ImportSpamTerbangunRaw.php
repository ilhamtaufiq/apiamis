<?php

namespace App\Console\Commands;

use App\Services\SpamTerbangunRawImportService;
use Illuminate\Console\Command;

class ImportSpamTerbangunRaw extends Command
{
    protected $signature = 'spam-terbangun:import {file : Path file Excel} {--replace : Hapus data raw lama sebelum import}';

    protected $description = 'Import data raw SPAM terbangun dari workbook Excel';

    public function handle(SpamTerbangunRawImportService $importer): int
    {
        $file = (string) $this->argument('file');

        if (! is_file($file)) {
            $this->error("File tidak ditemukan: {$file}");

            return self::FAILURE;
        }

        $result = $importer->import($file, (bool) $this->option('replace'));

        $this->info("Import selesai. {$result['imported']} baris disimpan.");

        return self::SUCCESS;
    }
}
