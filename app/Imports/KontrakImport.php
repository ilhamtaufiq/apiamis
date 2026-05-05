<?php

namespace App\Imports;

use App\Models\Kontrak;
use App\Models\Pekerjaan;
use App\Models\Penyedia;
use Maatwebsite\Excel\Concerns\ToModel;
use Maatwebsite\Excel\Concerns\WithHeadingRow;
use Maatwebsite\Excel\Concerns\WithValidation;
use Maatwebsite\Excel\Concerns\SkipsEmptyRows;
use Maatwebsite\Excel\Concerns\SkipsOnFailure;
use Maatwebsite\Excel\Concerns\SkipsFailures;
use Carbon\Carbon;

class KontrakImport implements ToModel, WithHeadingRow, WithValidation, SkipsEmptyRows, SkipsOnFailure
{
    use SkipsFailures;

    public function model(array $row)
    {
        // Resolve Pekerjaan (Nama Paket)
        $pekerjaanName = $row['nama_paket'] ?? null;
        $pekerjaan = null;
        if ($pekerjaanName) {
            $pekerjaan = Pekerjaan::where('nama_paket', 'LIKE', '%' . $pekerjaanName . '%')->first();
        }

        // Resolve Penyedia (Nama Penyedia)
        $penyediaName = $row['nama_penyedia'] ?? null;
        $penyedia = null;
        if ($penyediaName) {
            $penyedia = Penyedia::where('nama', 'LIKE', '%' . $penyediaName . '%')->first();
        }

        if (!$pekerjaan || !$penyedia) {
            return null;
        }

        return Kontrak::updateOrCreate(
            ['id_pekerjaan' => $pekerjaan->id],
            [
                'id_penyedia'       => $penyedia->id,
                'id_kegiatan'       => $pekerjaan->kegiatan_id,
                'kode_rup'          => $row['kode_rup'] ?? null,
                'kode_paket'        => $row['kode_paket'] ?? null,
                'nomor_penawaran'   => $row['nomor_penawaran'] ?? null,
                'tanggal_penawaran' => $this->parseDate($row['tanggal_penawaran'] ?? null),
                'nilai_kontrak'     => $this->parseNumber($row['nilai_kontrak'] ?? 0),
                'tgl_sppbj'         => $this->parseDate($row['tanggal_sppbj'] ?? null),
                'tgl_spk'           => $this->parseDate($row['tanggal_spk'] ?? null),
                'tgl_spmk'          => $this->parseDate($row['tanggal_spmk'] ?? null),
                'tgl_selesai'       => $this->parseDate($row['tanggal_selesai_kontrak'] ?? null),
                'sppbj'             => $row['nomor_sppbj'] ?? null,
                'spk'               => $row['nomor_spk'] ?? null,
                'spmk'              => $row['nomor_spmk'] ?? null,
            ]
        );
    }

    private function parseDate($value)
    {
        if (!$value) return null;
        try {
            if (is_numeric($value)) {
                return Carbon::instance(\PhpOffice\PhpSpreadsheet\Shared\Date::excelToDateTimeObject($value));
            }
            return Carbon::parse($value);
        } catch (\Exception $e) {
            return null;
        }
    }

    private function parseNumber($value)
    {
        if (is_numeric($value)) {
            return (float) $value;
        }
        $clean = preg_replace('/[^0-9.]/', '', str_replace(',', '.', $value));
        return (float) $clean;
    }

    public function rules(): array
    {
        return [
            'nama_paket' => 'required',
            'nama_penyedia' => 'required',
            'nomor_spk' => 'required',
            'nilai_kontrak' => 'required',
        ];
    }
}
