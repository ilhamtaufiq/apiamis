<?php

namespace App\Exports;

use App\Models\UsulanKegiatan;
use Maatwebsite\Excel\Concerns\FromQuery;
use Maatwebsite\Excel\Concerns\Exportable;
use Maatwebsite\Excel\Concerns\WithHeadings;
use Maatwebsite\Excel\Concerns\WithMapping;
use Maatwebsite\Excel\Concerns\WithStyles;
use Maatwebsite\Excel\Concerns\ShouldAutoSize;
use PhpOffice\PhpSpreadsheet\Worksheet\Worksheet;

class UsulanKegiatanExport implements FromQuery, WithHeadings, WithMapping, WithStyles, ShouldAutoSize
{
    use Exportable;

    public function query()
    {
        $query = UsulanKegiatan::with(['user', 'kecamatan', 'desa'])
            ->latest();

        return $query;
    }

    public function headings(): array
    {
        return [
            'No',
            'Sub Bidang',
            'Nama Pengusul',
            'Kecamatan',
            'Desa',
            'Perihal',
            'Tanggal Surat Masuk',
            'Nomor Surat Masuk',
            'Tanggal Surat',
            'Tanggal Pengajuan',
            'Dokumen',
        ];
    }

    public function map($usulan): array
    {
        static $no = 0;
        $no++;

        return [
            $no,
            $usulan->sub_bidang,
            $usulan->nama_pengusul,
            $usulan->kecamatan?->nama_kecamatan,
            $usulan->desa?->nama_desa,
            $usulan->perihal,
            $usulan->tanggal_surat_masuk?->format('d/m/Y'),
            $usulan->nomor_surat_masuk ?? '-',
            $usulan->tanggal_surat?->format('d/m/Y'),
            $usulan->created_at?->format('d/m/Y H:i'),
            $usulan->getFirstMediaUrl('dokumen') ?: '-',
        ];
    }

    public function styles(Worksheet $sheet)
    {
        return [
            1 => ['font' => ['bold' => true]],
        ];
    }
}
