<?php

namespace App\Exports;

use App\Models\Kontrak;
use Maatwebsite\Excel\Concerns\FromQuery;
use Maatwebsite\Excel\Concerns\Exportable;
use Maatwebsite\Excel\Concerns\WithHeadings;
use Maatwebsite\Excel\Concerns\WithMapping;
use Maatwebsite\Excel\Concerns\WithStyles;
use Maatwebsite\Excel\Concerns\ShouldAutoSize;
use PhpOffice\PhpSpreadsheet\Worksheet\Worksheet;

class KontrakExport implements FromQuery, WithHeadings, WithMapping, WithStyles, ShouldAutoSize
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
        $query = Kontrak::with(['kegiatan', 'pekerjaan.kecamatan', 'pekerjaan.kegiatan', 'penyedia'])->latest();

        if ($this->tahun) {
            $query->whereHas('kegiatan', function($q) {
                $q->where('tahun_anggaran', $this->tahun);
            });
        }

        if ($this->search) {
            $search = $this->search;
            $query->where(function($q) use ($search) {
                $q->where('kode_rup', 'like', "%{$search}%")
                  ->orWhere('nomor_penawaran', 'like', "%{$search}%")
                  ->orWhere('kode_paket', 'like', "%{$search}%")
                  ->orWhereHas('pekerjaan', function($q) use ($search) {
                      $q->where('nama_paket', 'like', "%{$search}%");
                  })
                  ->orWhereHas('penyedia', function($q) use ($search) {
                      $q->where('nama', 'like', "%{$search}%");
                  });
            });
        }

        return $query;
    }

    public function headings(): array
    {
        return [
            'Nama Pekerjaan',
            'Kode Rekening',
            'Pagu',
            'Sumber Dana',
            'Penyedia',
            'Nilai Kontrak',
            'Nomor SPK',
            'Tanggal SPK',
            'Nomor SPMK',
            'Tanggal SPMK',
            'Masa Pelaksanaan (Hari)',
            'Tanggal Selesai'
        ];
    }

    public function map($kontrak): array
    {
        $masa = '-';
        if ($kontrak->tgl_spmk && $kontrak->tgl_selesai) {
            $start = new \DateTime($kontrak->tgl_spmk);
            $end = new \DateTime($kontrak->tgl_selesai);
            $masa = $start->diff($end)->days + 1;
        }

        return [
            $kontrak->pekerjaan?->nama_paket,
            $kontrak->pekerjaan?->kode_rekening,
            $kontrak->pekerjaan?->pagu,
            $kontrak->pekerjaan?->kegiatan?->sumber_dana,
            $kontrak->penyedia?->nama,
            $kontrak->nilai_kontrak,
            $kontrak->spk,
            $kontrak->tgl_spk,
            $kontrak->spmk,
            $kontrak->tgl_spmk,
            $masa,
            $kontrak->tgl_selesai,
        ];
    }

    public function styles(Worksheet $sheet)
    {
        return [
            1    => ['font' => ['bold' => true]],
        ];
    }
}
