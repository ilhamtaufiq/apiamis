<?php

namespace App\Exports;

use App\Models\Kecamatan;
use App\Models\Kegiatan;
use Maatwebsite\Excel\Concerns\WithMultipleSheets;
use Maatwebsite\Excel\Concerns\FromCollection;
use Maatwebsite\Excel\Concerns\WithTitle;
use Maatwebsite\Excel\Concerns\WithHeadings;

class PekerjaanTemplateExport implements WithMultipleSheets
{
    public function sheets(): array
    {
        return [
            new TemplateSheet(),
            new KecamatanDesaRefSheet(),
            new KegiatanRefSheet(),
        ];
    }
}

class TemplateSheet implements FromCollection, WithTitle, WithHeadings
{
    public function collection()
    {
        return collect([]);
    }

    public function title(): string
    {
        return 'Template';
    }

    public function headings(): array
    {
        return [
            'Kode Rekening',
            'Nama Paket',
            'Kecamatan',
            'Desa',
            'Kegiatan',
            'Tahun',
            'Pagu',
        ];
    }
}

class KecamatanDesaRefSheet implements FromCollection, WithTitle, WithHeadings
{
    public function collection()
    {
        $data = [];
        $kecamatans = Kecamatan::with('desa')->get();

        foreach ($kecamatans as $kecamatan) {
            foreach ($kecamatan->desa as $desa) {
                $data[] = [
                    'kecamatan_name' => $kecamatan->n_kec,
                    'desa_name' => $desa->n_desa,
                ];
            }
        }

        return collect($data);
    }

    public function title(): string
    {
        return 'Ref Kecamatan & Desa';
    }

    public function headings(): array
    {
        return [
            'Nama Kecamatan',
            'Nama Desa',
        ];
    }
}

class KegiatanRefSheet implements FromCollection, WithTitle, WithHeadings
{
    public function collection()
    {
        return Kegiatan::select('tahun_anggaran', 'nama_sub_kegiatan')->get();
    }

    public function title(): string
    {
        return 'Ref Kegiatan';
    }

    public function headings(): array
    {
        return [
            'Tahun Anggaran',
            'Nama Sub Kegiatan',
        ];
    }
}
