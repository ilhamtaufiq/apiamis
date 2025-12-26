<?php

namespace App\Imports;

use App\Models\Pekerjaan;
use App\Models\Kecamatan;
use App\Models\Desa;
use App\Models\Kegiatan;
use Maatwebsite\Excel\Concerns\ToModel;
use Maatwebsite\Excel\Concerns\WithHeadingRow;
use Maatwebsite\Excel\Concerns\WithValidation;
use Maatwebsite\Excel\Concerns\SkipsEmptyRows;
use Maatwebsite\Excel\Concerns\SkipsOnFailure;
use Maatwebsite\Excel\Concerns\SkipsFailures;
use Maatwebsite\Excel\Concerns\WithMultipleSheets;
use Illuminate\Support\Facades\Log;

class PekerjaanImport implements WithMultipleSheets
{
    use SkipsFailures;

    public function sheets(): array
    {
        return [
            0 => new PekerjaanSheetImport(),
        ];
    }
}

class PekerjaanSheetImport implements ToModel, WithHeadingRow, WithValidation, SkipsEmptyRows, SkipsOnFailure
{
    use SkipsFailures;

    /**
     * @param array $row
     *
     * @return \Illuminate\Database\Eloquent\Model|null
     */
    public function model(array $row)
    {
        // Resolve Kecamatan
        $kecamatanName = $row['kecamatan'] ?? null;
        $kecamatan = null;
        if ($kecamatanName) {
            $kecamatan = Kecamatan::where('n_kec', 'LIKE', '%' . $kecamatanName . '%')->first();
        }

        // Resolve Desa
        $desaName = $row['desa'] ?? null;
        $desa = null;
        if ($desaName && $kecamatan) {
            $desa = Desa::where('kecamatan_id', $kecamatan->id)
                ->where('n_desa', 'LIKE', '%' . $desaName . '%')
                ->first();
        }

        // Resolve Kegiatan
        $kegiatanName = $row['kegiatan'] ?? null;
        $tahun = $row['tahun'] ?? null;
        $kegiatan = null;
        if ($kegiatanName && $tahun) {
            $kegiatan = Kegiatan::where('tahun_anggaran', $tahun)
                ->where('nama_sub_kegiatan', 'LIKE', '%' . $kegiatanName . '%')
                ->first();
        }

        return new Pekerjaan([
            'kode_rekening' => $row['kode_rekening'] ?? null,
            'nama_paket'    => $row['nama_paket'],
            'kecamatan_id'  => $kecamatan?->id,
            'desa_id'       => $desa?->id,
            'kegiatan_id'   => $kegiatan?->id,
            'pagu'          => $this->parsePagu($row['pagu'] ?? 0),
        ]);
    }

    /**
     * Clean and parse pagu value
     */
    private function parsePagu($value)
    {
        if (is_numeric($value)) {
            return (float) $value;
        }
        
        // Remove currency symbols, commas, and dots but keep decimal
        $clean = preg_replace('/[^0-9.]/', '', str_replace(',', '.', $value));
        return (float) $clean;
    }

    public function rules(): array
    {
        return [
            'nama_paket' => [
                'required',
                'string',
            ],
            'kecamatan'  => [
                'required',
                'string',
            ],
            'desa'       => [
                'required',
                'string',
            ],
        ];
    }

    /**
     * @param array $row
     * @return array
     */
    public function prepareForValidation($row, $index)
    {
        // Trim all strings in the row
        return array_map(function ($value) {
            return is_string($value) ? trim($value) : $value;
        }, $row);
    }
}
