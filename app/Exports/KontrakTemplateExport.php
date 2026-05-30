<?php

namespace App\Exports;

use App\Models\Pekerjaan;
use App\Models\Penyedia;
use Maatwebsite\Excel\Concerns\FromCollection;
use Maatwebsite\Excel\Concerns\WithHeadings;
use Maatwebsite\Excel\Concerns\WithTitle;
use Maatwebsite\Excel\Concerns\WithMultipleSheets;
use Illuminate\Support\Collection;

class KontrakTemplateExport implements WithMultipleSheets
{
    protected $tahun;

    public function __construct($tahun = null)
    {
        $this->tahun = $tahun;
    }

    public function sheets(): array
    {
        return [
            new KontrakTemplateSheet(),
            new PekerjaanRefSheet($this->tahun),
            new PenyediaRefSheet(),
        ];
    }
}

class KontrakTemplateSheet implements FromCollection, WithHeadings, WithTitle
{
    public function collection()
    {
        return new Collection([]);
    }

    public function headings(): array
    {
        return [
            'Nama Paket (pisahkan dengan koma jika konsolidasi)',
            'Nama Penyedia',
            'Kode RUP',
            'Kode Paket',
            'Nomor Penawaran',
            'Tanggal Penawaran',
            'Nilai Kontrak',
            'Tanggal SPPBJ',
            'Nomor SPPBJ',
            'Tanggal SPK',
            'Nomor SPK',
            'Tanggal SPMK',
            'Nomor SPMK',
            'Tanggal Selesai Kontrak',
        ];
    }

    public function title(): string
    {
        return 'Import Kontrak';
    }
}

class PekerjaanRefSheet implements FromCollection, WithHeadings, WithTitle
{
    protected $tahun;

    public function __construct($tahun = null)
    {
        $this->tahun = $tahun;
    }

    public function collection()
    {
        $query = Pekerjaan::select('nama_paket', 'kode_rekening', 'pagu');
        
        if ($this->tahun) {
            $query->whereHas('kegiatan', function($q) {
                $q->where('tahun_anggaran', $this->tahun);
            });
        }

        return $query->get();
    }

    public function headings(): array
    {
        return ['Nama Paket', 'Kode Rekening', 'Pagu'];
    }

    public function title(): string
    {
        return 'Referensi Pekerjaan';
    }
}

class PenyediaRefSheet implements FromCollection, WithHeadings, WithTitle
{
    public function collection()
    {
        return Penyedia::select('nama', 'alamat')->get();
    }

    public function headings(): array
    {
        return ['Nama Penyedia', 'Alamat'];
    }

    public function title(): string
    {
        return 'Referensi Penyedia';
    }
}
