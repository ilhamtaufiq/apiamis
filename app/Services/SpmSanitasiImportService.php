<?php

namespace App\Services;

use App\Models\Desa;
use App\Models\Kecamatan;
use App\Models\SpmSanitasi;
use PhpOffice\PhpSpreadsheet\IOFactory;

class SpmSanitasiImportService
{
    public int $importedRows = 0;
    public int $skippedRows = 0;
    /** @var array<int, string> */
    public array $errors = [];

    public function import(string $filePath, bool $replaceExisting = false): void
    {
        $spreadsheet = IOFactory::load($filePath);

        if ($replaceExisting) {
            SpmSanitasi::query()->delete();
        }

        $sheetMap = [
            'SPALDT' => 'spaldt',
            'SPALDS' => 'spalds',
            'IPLT' => 'iplt',
        ];

        foreach ($sheetMap as $sheetName => $jenis) {
            $sheet = $spreadsheet->getSheetByName($sheetName);
            if (!$sheet) {
                continue;
            }

            $rows = $sheet->toArray(null, true, true, false);
            foreach ($rows as $index => $row) {
                if ($this->shouldSkipRow($row, $index)) {
                    continue;
                }

                try {
                    $payload = $this->mapRow($jenis, $row);
                    if (!$payload) {
                        $this->skippedRows++;
                        continue;
                    }

                    SpmSanitasi::create($payload);
                    $this->importedRows++;
                } catch (\Throwable $e) {
                    $this->errors[] = "Sheet {$sheetName} baris ".($index + 1).': '.$e->getMessage();
                }
            }
        }
    }

    private function shouldSkipRow(array $row, int $index): bool
    {
        if ($index < 4) {
            return true;
        }

        $first = trim((string) ($row[0] ?? ''));
        if ($first === '' || str_starts_with($first, '(') || stripos($first, 'no') === 0) {
            return true;
        }

        if (!is_numeric($first)) {
            return true;
        }

        return false;
    }

    private function mapRow(string $jenis, array $row): ?array
    {
        if ($jenis === 'iplt') {
            return $this->mapIpltRow($row);
        }

        return $this->mapSpaldRow($jenis, $row);
    }

    private function mapSpaldRow(string $jenis, array $row): ?array
    {
        $kecamatan = trim((string) ($row[4] ?? ''));
        $desa = trim((string) ($row[5] ?? ''));
        $nama = trim((string) ($row[6] ?? ''));

        if ($nama === '') {
            return null;
        }

        $desaId = $this->resolveDesaId($kecamatan, $desa);
        $tahunIndex = $jenis === 'spaldt' ? 12 : 11;
        $jiwaIndex = $jenis === 'spaldt' ? 11 : null;

        return [
            'jenis' => $jenis,
            'desa_id' => $desaId,
            'skala_pelayanan' => $this->nullableString($row[1] ?? null),
            'nama_infrastruktur' => $nama,
            'latitude' => $this->nullableFloat($row[7] ?? null),
            'longitude' => $this->nullableFloat($row[8] ?? null),
            'alamat_lengkap' => $this->nullableString($row[9] ?? null),
            'jumlah_pemanfaat_kk' => $this->nullableInt($row[10] ?? null),
            'jumlah_pemanfaat_jiwa' => $jiwaIndex !== null ? $this->nullableInt($row[$jiwaIndex] ?? null) : null,
            'tahun_konstruksi' => $this->nullableInt($row[$tahunIndex] ?? null),
            'pembiayaan_apbn' => $this->nullableMoney($row[$tahunIndex + 1] ?? null),
            'pembiayaan_apbd' => $this->nullableMoney($row[$tahunIndex + 2] ?? null),
            'pembiayaan_dak' => $this->nullableMoney($row[$tahunIndex + 3] ?? null),
            'pembiayaan_hibah' => $this->nullableMoney($row[$tahunIndex + 4] ?? null),
            'pembiayaan_csr' => $this->nullableMoney($row[$tahunIndex + 5] ?? null),
            'pembiayaan_lain' => $this->nullableMoney($row[$tahunIndex + 6] ?? null),
            'pembiayaan_total' => $this->nullableMoney($row[$tahunIndex + 7] ?? null),
            'status_keberfungsian' => $this->nullableString($row[$tahunIndex + 8] ?? null),
            'kualitas_keberfungsian' => $this->nullableString($row[$tahunIndex + 9] ?? null),
            'pengelola' => $this->nullableString($row[$tahunIndex + 10] ?? null),
            'kapasitas_desain' => $this->nullableFloat($row[$tahunIndex + 11] ?? null),
            'kapasitas_terpakai' => $this->nullableFloat($row[$tahunIndex + 12] ?? null),
            'kapasitas_tidak_terpakai' => $this->nullableFloat($row[$tahunIndex + 13] ?? null),
            'jenis_pengolahan' => $this->nullableString($row[$tahunIndex + 14] ?? null),
            'peta_cakupan' => $this->nullableString($row[$tahunIndex + 15] ?? null),
            'status_lahan' => $this->nullableString($row[$tahunIndex + 16] ?? null),
            'luas_lahan_ha' => $this->nullableString($row[$tahunIndex + 17] ?? null),
            'opsi_teknologi' => $this->nullableString($row[$tahunIndex + 18] ?? null),
            'jumlah_stasiun_pompa' => $this->nullableString($row[$tahunIndex + 19] ?? null),
            'biaya_operasional' => $this->nullableMoney($row[$tahunIndex + 20] ?? null),
        ];
    }

    private function mapIpltRow(array $row): ?array
    {
        $kecamatan = trim((string) ($row[3] ?? ''));
        $desa = trim((string) ($row[4] ?? ''));
        $nama = trim((string) ($row[5] ?? ''));

        if ($nama === '') {
            return null;
        }

        $desaId = $this->resolveDesaId($kecamatan, $desa);
        $tahun = $this->nullableInt($row[8] ?? null) ?? $this->nullableInt($row[22] ?? null);

        return [
            'jenis' => 'iplt',
            'desa_id' => $desaId,
            'nama_infrastruktur' => $nama,
            'latitude' => $this->nullableFloat($row[6] ?? null),
            'longitude' => $this->nullableFloat($row[7] ?? null),
            'tahun_konstruksi' => $tahun,
            'jenis_pengelola' => $this->nullableString($row[9] ?? null),
            'kapasitas_desain' => $this->nullableFloat($row[10] ?? null),
            'kapasitas_terpakai' => $this->nullableFloat($row[11] ?? null),
            'kapasitas_tidak_terpakai' => $this->nullableFloat($row[12] ?? null),
            'status_keberfungsian' => $this->nullableString($row[13] ?? null),
            'kualitas_keberfungsian' => $this->nullableString($row[14] ?? null),
            'sistem_pengolahan' => $this->nullableString($row[15] ?? null),
            'truk_tinja_unit' => $this->nullableInt($row[16] ?? null),
            'kapasitas_truk_m3' => $this->nullableFloat($row[17] ?? null),
            'jumlah_ritasi' => $this->nullableInt($row[18] ?? null),
            'jarak_maksimal_pelayanan_km' => $this->nullableFloat($row[19] ?? null),
            'alokasi_biaya_operasional' => $this->nullableMoney($row[20] ?? null),
            'jumlah_pemanfaat_kk' => $this->nullableInt($row[21] ?? null),
            'pembiayaan_apbn' => $this->nullableMoney($row[23] ?? null),
            'pembiayaan_apbd' => $this->nullableMoney($row[24] ?? null),
            'pembiayaan_dak' => $this->nullableMoney($row[25] ?? null),
            'pembiayaan_hibah' => $this->nullableMoney($row[26] ?? null),
            'pembiayaan_csr' => $this->nullableMoney($row[27] ?? null),
            'pembiayaan_lain' => $this->nullableMoney($row[28] ?? null),
            'pembiayaan_total' => $this->nullableMoney($row[29] ?? null),
        ];
    }

    private function resolveDesaId(string $kecamatanName, string $desaName): ?int
    {
        if ($kecamatanName === '' || $desaName === '') {
            return null;
        }

        $kecamatan = Kecamatan::query()
            ->whereRaw('LOWER(n_kec) = ?', [mb_strtolower($kecamatanName)])
            ->first();

        if (!$kecamatan) {
            return null;
        }

        $desa = Desa::query()
            ->where('kecamatan_id', $kecamatan->id)
            ->whereRaw('LOWER(n_desa) = ?', [mb_strtolower($desaName)])
            ->first();

        return $desa?->id;
    }

    private function nullableString(mixed $value): ?string
    {
        $text = trim((string) $value);
        return $text === '' || $text === '-' ? null : $text;
    }

    private function nullableInt(mixed $value): ?int
    {
        if ($value === null || $value === '') {
            return null;
        }

        if (is_numeric($value)) {
            return (int) $value;
        }

        return null;
    }

    private function nullableFloat(mixed $value): ?float
    {
        if ($value === null || $value === '') {
            return null;
        }

        if (is_numeric($value)) {
            return (float) $value;
        }

        $normalized = str_replace(',', '.', preg_replace('/[^0-9,.-]/', '', (string) $value) ?? '');
        return $normalized !== '' && is_numeric($normalized) ? (float) $normalized : null;
    }

    private function nullableMoney(mixed $value): ?float
    {
        return $this->nullableFloat($value);
    }
}