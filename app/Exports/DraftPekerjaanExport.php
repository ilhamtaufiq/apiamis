<?php

namespace App\Exports;

use App\Models\Pekerjaan;
use Maatwebsite\Excel\Concerns\FromQuery;
use Maatwebsite\Excel\Concerns\Exportable;
use Maatwebsite\Excel\Concerns\WithHeadings;
use Maatwebsite\Excel\Concerns\WithMapping;
use Maatwebsite\Excel\Concerns\WithStyles;
use Maatwebsite\Excel\Concerns\ShouldAutoSize;
use PhpOffice\PhpSpreadsheet\Worksheet\Worksheet;

class DraftPekerjaanExport implements FromQuery, WithHeadings, WithMapping, WithStyles, ShouldAutoSize
{
    use Exportable;

    protected $tahun;
    protected $search;

    public function __construct($tahun = null, $search = null)
    {
        $this->tahun = $tahun;
        $this->search = $search;
    }

    public function query()
    {
        $query = Pekerjaan::with(['kecamatan', 'desa', 'draft.penyedia', 'kegiatan'])
            ->byUserRole();

        if ($this->tahun) {
            $query->whereHas('kegiatan', function($q) {
                $q->where('tahun_anggaran', $this->tahun);
            });
        }

        if ($this->search) {
            $searchTerm = $this->search;
            $query->where(function($q) use ($searchTerm) {
                $q->where('nama_paket', 'LIKE', '%' . $searchTerm . '%')
                  ->orWhere('kode_rekening', 'LIKE', '%' . $searchTerm . '%');
            });
        }

        return $query;
    }

    public function headings(): array
    {
        return [
            'Nama Pekerjaan',
            'Kode Rekening',
            'Kecamatan',
            'Desa',
            'Pagu',
            'Kode RUP',
            'Kode Paket',
            'Nama Pelaksana',
            'Penyedia'
        ];
    }

    public function map($pekerjaan): array
    {
        return [
            $pekerjaan->nama_paket,
            $pekerjaan->kode_rekening,
            $pekerjaan->kecamatan?->nama_kecamatan,
            $pekerjaan->desa?->nama_desa,
            $pekerjaan->pagu,
            $pekerjaan->draft?->kode_rup,
            $pekerjaan->draft?->kode_paket,
            $pekerjaan->draft?->nama_pelaksana,
            $pekerjaan->draft?->penyedia?->nama,
        ];
    }

    public function styles(Worksheet $sheet)
    {
        return [
            1    => ['font' => ['bold' => true]],
        ];
    }
}
