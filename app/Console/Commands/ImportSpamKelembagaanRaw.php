<?php

namespace App\Console\Commands;

use App\Services\SpamKelembagaanRawImportService;
use Illuminate\Console\Command;

class ImportSpamKelembagaanRaw extends Command
{
    protected $signature = 'spam-kelembagaan:import {file : Path file Excel} {--replace : Hapus data raw lama sebelum import}';

    protected $description = 'Import data raw kelembagaan SPAM dari sheet JP dan BJP';

    public function handle(SpamKelembagaanRawImportService $importer): int
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
