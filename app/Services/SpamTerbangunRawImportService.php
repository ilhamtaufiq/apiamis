<?php

namespace App\Services;

use App\Models\SpamTerbangunRaw;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\DB;
use PhpOffice\PhpSpreadsheet\IOFactory;
use PhpOffice\PhpSpreadsheet\Shared\Date as ExcelDate;
use PhpOffice\PhpSpreadsheet\Worksheet\Worksheet;
use Throwable;

class SpamTerbangunRawImportService
{
    /**
     * @return array{imported: int, replaced: bool}
     */
    public function import(UploadedFile|string $file, bool $replace = false, ?string $sourceFile = null): array
    {
        $path = $file instanceof UploadedFile ? $file->getRealPath() : $file;
        $sourceFile ??= $file instanceof UploadedFile ? $file->getClientOriginalName() : basename($file);
        $records = $this->parseMainDataSheet($path, $sourceFile);

        DB::transaction(function () use ($records, $replace) {
            if ($replace) {
                SpamTerbangunRaw::query()->delete();
            }

            foreach (array_chunk($records, 100) as $chunk) {
                SpamTerbangunRaw::insert($chunk);
            }
        });

        return [
            'imported' => count($records),
            'replaced' => $replace,
        ];
    }

    /**
     * @return array<int, array<string, mixed>>
     */
    private function parseMainDataSheet(string $path, string $sourceFile): array
    {
        $reader = IOFactory::createReaderForFile($path);
        $reader->setReadDataOnly(true);
        $spreadsheet = $reader->load($path);
        $sheet = $spreadsheet->getSheetByName('Main data') ?? $spreadsheet->getSheet(0);

        $records = [];
        $currentKecamatan = null;
        $highestRow = min($sheet->getHighestDataRow(), 1000);
        $now = now();

        for ($row = 10; $row <= $highestRow; $row++) {
            $values = $this->rowValues($sheet, $row, 23);
            $desaMarker = $this->cleanString($values[1] ?? null);
            $desaKelurahan = $this->cleanString($values[3] ?? null);
            $pengelola = $this->cleanString($values[4] ?? null);

            if ($desaMarker && str_starts_with(strtolower($desaMarker), 'kecamatan')) {
                $currentKecamatan = trim(str_ireplace('Kecamatan', '', $desaMarker));

                continue;
            }

            if ((! $desaKelurahan && ! $pengelola) || str_starts_with(strtolower((string) $desaMarker), 'jumlah')) {
                continue;
            }

            $yearRaw = $this->cleanString($values[19] ?? null);
            [$yearStart, $yearEnd] = $this->parseYears($yearRaw);

            $record = [
                'kecamatan' => $currentKecamatan,
                'jenis_wilayah' => in_array($desaMarker, ['Desa', 'Kel'], true) ? $desaMarker : null,
                'desa_kelurahan' => $desaKelurahan,
                'nama_pengelola' => $pengelola,
                'sumber_air_baku' => $this->cleanString($values[5] ?? null),
                'sistem_aliran' => $this->cleanString($values[6] ?? null),
                'debit_sumber_l_det' => $this->toDecimal($values[7] ?? null),
                'debit_diambil_l_det' => $this->toDecimal($values[8] ?? null),
                'penduduk_terlayani' => $this->toInteger($values[9] ?? null),
                'jumlah_penduduk' => $this->toInteger($values[10] ?? null),
                'hu_ku_unit' => $this->toInteger($values[11] ?? null),
                'sr_unit' => $this->toInteger($values[12] ?? null),
                'tanpa_meteran_air_unit' => $this->toInteger($values[13] ?? null),
                'sumber_dana_raw' => $this->cleanString($values[14] ?? null),
                'asal_proyek' => $this->cleanString($values[15] ?? null),
                'nilai_dak_apbn_rp' => $this->toDecimal($values[16] ?? null),
                'nilai_apbd_rp' => $this->toDecimal($values[17] ?? null),
                'nilai_banprov_rp' => $this->toDecimal($values[18] ?? null),
                'tahun_pembangunan_raw' => $yearRaw,
                'tahun_pembangunan_awal' => $yearStart,
                'tahun_pembangunan_akhir' => $yearEnd,
                'kondisi_raw' => $this->cleanString($values[20] ?? null),
                'kondisi_normalized' => $this->normalizeCondition($values[20] ?? null),
                'tanggal_terakhir_laporan' => $this->toDate($values[21] ?? null),
                'keterangan' => $this->cleanString($values[22] ?? null),
                'raw_payload' => json_encode($values, JSON_UNESCAPED_UNICODE),
                'source_file' => $sourceFile,
                'source_sheet' => 'Main data',
                'source_row' => $row,
                'created_at' => $now,
                'updated_at' => $now,
            ];

            if (count(array_filter($record, fn ($value) => $value !== null && $value !== '' && ! is_array($value))) >= 5) {
                $records[] = $record;
            }
        }

        return $records;
    }

    /**
     * @return array<int, mixed>
     */
    private function rowValues(Worksheet $sheet, int $row, int $columns): array
    {
        $values = [];

        for ($column = 1; $column <= $columns; $column++) {
            $cell = $sheet->getCell([$column, $row]);
            $value = $cell->getValue();

            if (is_string($value) && str_starts_with($value, '=')) {
                try {
                    $value = $cell->getCalculatedValue();
                } catch (Throwable) {
                    $value = $cell->getOldCalculatedValue() ?: $cell->getValue();
                }
            }

            $values[] = $value;
        }

        return $values;
    }

    private function cleanString(mixed $value): ?string
    {
        if ($value === null) {
            return null;
        }

        $value = trim(preg_replace('/\s+/u', ' ', (string) $value) ?? '');

        return $value === '' ? null : $value;
    }

    private function toDecimal(mixed $value): ?float
    {
        if ($value === null || $value === '' || is_bool($value)) {
            return null;
        }

        if (is_numeric($value)) {
            return round((float) $value, 2);
        }

        if (is_string($value) && str_starts_with(trim($value), '=')) {
            return null;
        }

        $normalized = str_replace(',', '.', preg_replace('/[^0-9,\.\-]/', '', (string) $value) ?? '');

        return is_numeric($normalized) ? round((float) $normalized, 2) : null;
    }

    private function toInteger(mixed $value): ?int
    {
        $decimal = $this->toDecimal($value);

        return $decimal === null ? null : (int) round($decimal);
    }

    private function toDate(mixed $value): ?string
    {
        if ($value === null || $value === '' || $value === '#REF!') {
            return null;
        }

        try {
            if (is_numeric($value)) {
                return ExcelDate::excelToDateTimeObject((float) $value)->format('Y-m-d');
            }

            $timestamp = strtotime((string) $value);

            return $timestamp ? date('Y-m-d', $timestamp) : null;
        } catch (Throwable) {
            return null;
        }
    }

    /**
     * @return array{0: ?int, 1: ?int}
     */
    private function parseYears(?string $value): array
    {
        if (! $value) {
            return [null, null];
        }

        preg_match_all('/(?:19|20)\d{2}/', $value, $matches);
        $years = array_map('intval', $matches[0] ?? []);

        if ($years === []) {
            return [null, null];
        }

        return [min($years), max($years)];
    }

    private function normalizeCondition(mixed $value): ?string
    {
        $condition = strtolower($this->cleanString($value) ?? '');

        return match (true) {
            $condition === '' || $condition === '-' => null,
            str_contains($condition, 'tdk') || str_contains($condition, 'tidak') => 'tidak_berfungsi',
            str_contains($condition, 'fungsi') || str_contains($condition, 'befungsi') => 'berfungsi',
            default => 'lainnya',
        };
    }
}
