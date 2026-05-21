<?php

namespace App\Services;

use App\Models\SpamKelembagaanRaw;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\DB;
use PhpOffice\PhpSpreadsheet\IOFactory;
use PhpOffice\PhpSpreadsheet\Worksheet\Worksheet;
use Throwable;

class SpamKelembagaanRawImportService
{
    private const SHEETS = [
        'DATA KELEMBAGAAN JP' => 'JP',
        'DATA KELEMBAGAAN BJP' => 'BJP',
    ];

    public function import(UploadedFile|string $file, bool $replace = false, ?string $sourceFile = null): array
    {
        $path = $file instanceof UploadedFile ? $file->getRealPath() : $file;
        $sourceFile ??= $file instanceof UploadedFile ? $file->getClientOriginalName() : basename($file);
        $records = $this->parseWorkbook($path, $sourceFile);

        DB::transaction(function () use ($records, $replace) {
            if ($replace) {
                SpamKelembagaanRaw::query()->delete();
            }

            foreach (array_chunk($records, 100) as $chunk) {
                SpamKelembagaanRaw::insert($chunk);
            }
        });

        return [
            'imported' => count($records),
            'replaced' => $replace,
        ];
    }

    private function parseWorkbook(string $path, string $sourceFile): array
    {
        $reader = IOFactory::createReaderForFile($path);
        $reader->setReadDataOnly(true);
        $spreadsheet = $reader->load($path);
        $records = [];

        foreach (self::SHEETS as $sheetName => $type) {
            $sheet = $spreadsheet->getSheetByName($sheetName);

            if (! $sheet) {
                continue;
            }

            array_push($records, ...$this->parseSheet($sheet, $type, $sourceFile));
        }

        return $records;
    }

    private function parseSheet(Worksheet $sheet, string $type, string $sourceFile): array
    {
        $records = [];
        $currentKecamatan = null;
        $highestRow = min($sheet->getHighestDataRow(), 1000);
        $now = now();

        for ($row = 7; $row <= $highestRow; $row++) {
            $values = $this->rowValues($sheet, $row, max($sheet->getHighestDataColumn(), 'BF'));
            $map = $type === 'JP' ? $this->jpMap($values) : $this->bjpMap($values);

            if ($map['kecamatan']) {
                $currentKecamatan = $map['kecamatan'];
            }

            $kecamatan = $map['kecamatan'] ?: $currentKecamatan;
            $desa = $map['desa_kelurahan'];
            $pengelola = $map['nama_pengelola'];

            if (! $desa && ! $pengelola) {
                continue;
            }

            $yearRaw = $map['tahun_pembangunan_raw'];
            [$yearStart, $yearEnd] = $this->parseYears($yearRaw);
            $desaNormalized = $this->normalizeKey($desa);
            $kecamatanNormalized = $this->normalizeKey($kecamatan);

            $records[] = [
                ...$map,
                'jenis_jaringan' => $type,
                'kecamatan' => $kecamatan,
                'desa_kelurahan_normalized' => $desaNormalized,
                'lokasi_key' => $kecamatanNormalized && $desaNormalized ? "{$kecamatanNormalized}|{$desaNormalized}" : null,
                'tahun_pembangunan_awal' => $yearStart,
                'tahun_pembangunan_akhir' => $yearEnd,
                'raw_payload' => json_encode($values, JSON_UNESCAPED_UNICODE),
                'source_file' => $sourceFile,
                'source_sheet' => $sheet->getTitle(),
                'source_row' => $row,
                'created_at' => $now,
                'updated_at' => $now,
            ];
        }

        return $records;
    }

    private function jpMap(array $values): array
    {
        return [
            'kecamatan' => $this->cleanString($values[2] ?? null),
            'desa_kelurahan' => $this->cleanString($values[3] ?? null),
            'tahun_pembangunan_raw' => $this->cleanString($values[6] ?? null),
            'sumber_dana_raw' => $this->cleanString($values[7] ?? null),
            'program_pembangunan' => $this->cleanString($values[8] ?? null),
            'nama_pengelola' => $this->cleanString($values[9] ?? null),
            'perdes_pembentukan_pokmas' => $this->cleanString($values[10] ?? null),
            'pengurus_kepala' => $this->cleanString($values[11] ?? null),
            'pengurus_bendahara' => $this->cleanString($values[12] ?? null),
            'pengurus_sekretaris' => $this->cleanString($values[13] ?? null),
            'kapasitas_mata_air_l_det' => $this->toDecimal($values[14] ?? null),
            'sistem_aliran' => $this->cleanString($values[15] ?? null),
            'kapasitas_air_tanah_l_det' => $this->toDecimal($values[16] ?? null),
            'kapasitas_lain_l_det' => $this->toDecimal($values[17] ?? null),
            'dasar_hukum_tarif' => $this->cleanString($values[18] ?? null),
            'besaran_iuran' => $this->cleanString($values[19] ?? null),
            'pendapatan_bulanan_rp' => $this->toDecimal($values[20] ?? null),
            'biaya_operasional_bulanan_rp' => $this->toDecimal($values[21] ?? null),
            'sr_unit' => $this->toInteger($values[22] ?? null),
            'kk_terlayani' => $this->toInteger($values[23] ?? null),
            'jiwa_terlayani' => $this->toInteger($values[24] ?? null),
            'target_layanan' => $this->toInteger($values[43] ?? null),
        ];
    }

    private function bjpMap(array $values): array
    {
        return [
            'kecamatan' => $this->cleanString($values[2] ?? null),
            'desa_kelurahan' => $this->cleanString($values[3] ?? null),
            'tahun_pembangunan_raw' => $this->cleanString($values[4] ?? null),
            'sumber_dana_raw' => $this->cleanString($values[5] ?? null),
            'program_pembangunan' => $this->cleanString($values[6] ?? null),
            'nama_pengelola' => $this->cleanString($values[7] ?? null),
            'perdes_pembentukan_pokmas' => $this->cleanString($values[8] ?? null),
            'pengurus_kepala' => $this->cleanString($values[9] ?? null),
            'pengurus_bendahara' => $this->cleanString($values[10] ?? null),
            'pengurus_sekretaris' => $this->cleanString($values[11] ?? null),
            'kapasitas_mata_air_l_det' => $this->toDecimal($values[12] ?? null),
            'sistem_aliran' => $this->cleanString($values[13] ?? null),
            'kapasitas_air_tanah_l_det' => $this->toDecimal($values[14] ?? null),
            'kapasitas_lain_l_det' => $this->toDecimal($values[15] ?? null),
            'dasar_hukum_tarif' => $this->cleanString($values[16] ?? null),
            'besaran_iuran' => $this->cleanString($values[17] ?? null),
            'pendapatan_bulanan_rp' => $this->toDecimal($values[18] ?? null),
            'biaya_operasional_bulanan_rp' => $this->toDecimal($values[19] ?? null),
            'sr_unit' => $this->toInteger($values[20] ?? null),
            'kk_terlayani' => $this->toInteger($values[21] ?? null),
            'jiwa_terlayani' => $this->toInteger($values[22] ?? null),
            'target_layanan' => null,
        ];
    }

    private function rowValues(Worksheet $sheet, int $row, string $highestColumn): array
    {
        $highestIndex = \PhpOffice\PhpSpreadsheet\Cell\Coordinate::columnIndexFromString($highestColumn);
        $values = [];

        for ($column = 1; $column <= $highestIndex; $column++) {
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

        return $value === '' || $value === '-' ? null : $value;
    }

    private function toDecimal(mixed $value): ?float
    {
        if ($value === null || $value === '' || $value === '-' || is_bool($value)) {
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

    private function parseYears(?string $value): array
    {
        if (! $value) {
            return [null, null];
        }

        preg_match_all('/(?:19|20)\d{2}/', $value, $matches);
        $years = array_map('intval', $matches[0] ?? []);

        return $years === [] ? [null, null] : [min($years), max($years)];
    }

    private function normalizeKey(?string $value): ?string
    {
        if (! $value) {
            return null;
        }

        return strtolower(preg_replace('/[^a-z0-9]+/i', '', $value) ?? '');
    }
}
