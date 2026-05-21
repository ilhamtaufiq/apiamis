<?php

namespace App\Console\Commands;

use App\Models\Desa;
use Illuminate\Console\Command;
use PhpOffice\PhpSpreadsheet\IOFactory;

class ImportDesaPopulation extends Command
{
    protected $signature = 'desa:import-population {file : Path file Excel penduduk} {--dry-run : Cek matching tanpa update database}';

    protected $description = 'Update jumlah_penduduk tbl_desa berdasarkan file penduduk, dicocokkan dengan kecamatan + desa';

    public function handle(): int
    {
        $file = (string) $this->argument('file');

        if (! is_file($file)) {
            $this->error("File tidak ditemukan: {$file}");

            return self::FAILURE;
        }

        $lookup = $this->buildDesaLookup();
        $rows = $this->readRows($file);
        $matched = 0;
        $updated = 0;
        $unmatched = [];
        $ambiguous = [];

        foreach ($rows as $row) {
            $key = $this->normalize($row['kecamatan']).'|'.$this->normalize($row['desa']);
            $candidates = $lookup[$key] ?? [];

            if (count($candidates) === 0) {
                $unmatched[] = $row;

                continue;
            }

            if (count($candidates) > 1) {
                $ambiguous[] = $row;

                continue;
            }

            $matched++;
            $desa = $candidates[0];

            if (! $this->option('dry-run')) {
                $desa->update([
                    'jumlah_penduduk' => $row['jumlah_penduduk'],
                ]);
                $updated++;
            }
        }

        $this->info('Import penduduk selesai.');
        $this->line('Rows: '.count($rows));
        $this->line("Matched unik: {$matched}");
        $this->line('Updated: '.$updated);
        $this->line('Unmatched: '.count($unmatched));
        $this->line('Ambiguous: '.count($ambiguous));

        foreach (array_slice($unmatched, 0, 20) as $row) {
            $this->warn("UNMATCHED {$row['kecamatan']} / {$row['desa']} = {$row['jumlah_penduduk']}");
        }

        foreach (array_slice($ambiguous, 0, 20) as $row) {
            $this->warn("AMBIGUOUS {$row['kecamatan']} / {$row['desa']} = {$row['jumlah_penduduk']}");
        }

        return self::SUCCESS;
    }

    private function buildDesaLookup(): array
    {
        $lookup = [];

        Desa::with('kecamatan')->get()->each(function (Desa $desa) use (&$lookup) {
            $key = $this->normalize((string) $desa->kecamatan?->n_kec).'|'.$this->normalize((string) $desa->n_desa);
            $lookup[$key][] = $desa;
        });

        return $lookup;
    }

    private function readRows(string $file): array
    {
        $reader = IOFactory::createReaderForFile($file);
        $reader->setReadDataOnly(true);
        $spreadsheet = $reader->load($file);
        $sheet = $spreadsheet->getActiveSheet();
        $rows = [];

        for ($row = 2; $row <= $sheet->getHighestDataRow(); $row++) {
            $desa = $this->cleanString($sheet->getCell([2, $row])->getValue());
            $kecamatan = $this->cleanString($sheet->getCell([3, $row])->getValue());
            $population = $this->toInteger($sheet->getCell([4, $row])->getValue());

            if (! $desa || ! $kecamatan || $population === null) {
                continue;
            }

            $rows[] = [
                'desa' => $desa,
                'kecamatan' => $kecamatan,
                'jumlah_penduduk' => $population,
            ];
        }

        return $rows;
    }

    private function cleanString(mixed $value): ?string
    {
        if ($value === null) {
            return null;
        }

        $value = trim(preg_replace('/\s+/u', ' ', (string) $value) ?? '');

        return $value === '' ? null : $value;
    }

    private function toInteger(mixed $value): ?int
    {
        if ($value === null || $value === '') {
            return null;
        }

        return is_numeric($value) ? (int) round((float) $value) : null;
    }

    private function normalize(?string $value): ?string
    {
        if (! $value) {
            return null;
        }

        $value = str_ireplace(['kecamatan', 'desa', 'kelurahan'], '', $value);

        return strtolower(preg_replace('/[^a-z0-9]+/i', '', $value) ?? '');
    }
}
