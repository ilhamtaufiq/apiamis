<?php

namespace App\Imports;

use App\Models\Kontrak;
use App\Models\Pekerjaan;
use App\Models\Penyedia;
use Maatwebsite\Excel\Concerns\ToCollection;
use Maatwebsite\Excel\Concerns\WithHeadingRow;
use Maatwebsite\Excel\Concerns\SkipsEmptyRows;
use Illuminate\Support\Collection;
use Carbon\Carbon;

class KontrakImport implements ToCollection, WithHeadingRow, SkipsEmptyRows
{
    public $rows = 0;
    public $importedRows = 0;
    public $skippedRows = [];
    public $failures = [];

    public function collection(Collection $rows)
    {
        foreach ($rows as $index => $row) {
            // Check if row is mostly empty (nama_paket is required)
            if (!isset($row['nama_paket']) || empty(trim($row['nama_paket']))) {
                continue;
            }

            $this->rows++;
            $rowNumber = $index + 2; // +2 for heading row and 0-based index

            // Parse dates first to extract year for strict matching
            $tglSppbj = $this->parseDate($row['tanggal_sppbj'] ?? null);
            $tglSpk = $this->parseDate($row['tanggal_spk'] ?? null);
            $tglSpmk = $this->parseDate($row['tanggal_spmk'] ?? null);
            $tglSelesai = $this->parseDate($row['tanggal_selesai_kontrak'] ?? null);
            $tglPenawaran = $this->parseDate($row['tanggal_penawaran'] ?? null);

            // Determine reference year (prioritize SPK date, then SPMK, then SPPBJ)
            $refDate = $tglSpk ?? $tglSpmk ?? $tglSppbj;
            $refYear = $refDate ? $refDate->year : null;

            // Resolve Pekerjaan (Nama Paket) - STRICT EXACT MATCH + YEAR
            $pekerjaanName = isset($row['nama_paket']) ? trim($row['nama_paket']) : null;
            $pekerjaan = null;
            if ($pekerjaanName) {
                $query = Pekerjaan::where(function($q) use ($pekerjaanName) {
                    $q->where('nama_paket', $pekerjaanName)
                      ->orWhereRaw('LOWER(nama_paket) = ?', [strtolower($pekerjaanName)]);
                });

                if ($refYear) {
                    $query->whereHas('kegiatan', function($q) use ($refYear) {
                        $q->where('tahun_anggaran', $refYear);
                    });
                }

                $pekerjaan = $query->first();
            }

            // Resolve Penyedia (Nama Penyedia) - STRICT EXACT MATCH
            $penyediaName = isset($row['nama_penyedia']) ? trim($row['nama_penyedia']) : null;
            $penyedia = null;
            if ($penyediaName) {
                $penyedia = Penyedia::where('nama', $penyediaName)
                    ->orWhereRaw('LOWER(nama) = ?', [strtolower($penyediaName)])
                    ->first();
            }

            // Strict Validation
            if (!$pekerjaan || !$penyedia) {
                $reason = "";
                if (!$pekerjaan) {
                    if ($refYear) {
                        $reason .= "Pekerjaan '{$pekerjaanName}' tidak ditemukan pada tahun anggaran {$refYear}. ";
                    } else {
                        $reason .= "Pekerjaan '{$pekerjaanName}' tidak ditemukan (Tahun anggaran tidak dapat ditentukan dari tanggal SPK/SPMK). ";
                    }
                }
                if (!$penyedia) $reason .= "Penyedia '{$penyediaName}' tidak ditemukan. ";

                $this->failures[] = [
                    'row' => $rowNumber,
                    'attribute' => 'referensi',
                    'errors' => [$reason],
                    'values' => $row->toArray()
                ];
                continue;
            }

            try {
                Kontrak::updateOrCreate(
                    ['id_pekerjaan' => $pekerjaan->id],
                    [
                        'id_penyedia'       => $penyedia->id,
                        'id_kegiatan'       => $pekerjaan->kegiatan_id,
                        'kode_rup'          => $row['kode_rup'] ?? null,
                        'kode_paket'        => $row['kode_paket'] ?? null,
                        'nomor_penawaran'   => $row['nomor_penawaran'] ?? null,
                        'tanggal_penawaran' => $tglPenawaran,
                        'nilai_kontrak'     => $this->parseNumber($row['nilai_kontrak'] ?? 0),
                        'tgl_sppbj'         => $tglSppbj,
                        'tgl_spk'           => $tglSpk,
                        'tgl_spmk'          => $tglSpmk,
                        'tgl_selesai'       => $tglSelesai,
                        'sppbj'             => $row['nomor_sppbj'] ?? null,
                        'spk'               => $row['nomor_spk'] ?? null,
                        'spmk'              => $row['nomor_spmk'] ?? null,
                    ]
                );
                $this->importedRows++;
            } catch (\Exception $e) {
                $this->failures[] = [
                    'row' => $rowNumber,
                    'attribute' => 'database',
                    'errors' => [$e->getMessage()],
                    'values' => $row->toArray()
                ];
            }
        }
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

    public function failures()
    {
        return $this->failures;
    }
}
