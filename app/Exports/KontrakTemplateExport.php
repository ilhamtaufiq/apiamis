<?php

namespace App\Exports;

use App\Models\Pekerjaan;
use App\Models\Penyedia;
use Maatwebsite\Excel\Concerns\WithEvents;
use Maatwebsite\Excel\Concerns\FromCollection;
use Maatwebsite\Excel\Concerns\WithHeadings;
use Maatwebsite\Excel\Concerns\WithTitle;
use Maatwebsite\Excel\Concerns\WithMultipleSheets;
use Illuminate\Support\Collection;
use Maatwebsite\Excel\Events\AfterSheet;
use Maatwebsite\Excel\Concerns\ShouldAutoSize;
use Maatwebsite\Excel\Concerns\WithStyles;
use PhpOffice\PhpSpreadsheet\Worksheet\Worksheet;

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
            new KontrakValidationSheet(),
            new PekerjaanRefSheet($this->tahun),
            new PenyediaRefSheet(),
        ];
    }
}

class KontrakTemplateSheet implements FromCollection, WithHeadings, WithTitle, WithStyles, ShouldAutoSize
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

    public function styles(Worksheet $sheet)
    {
        return [
            1 => ['font' => ['bold' => true]],
        ];
    }
}

class KontrakValidationSheet implements FromCollection, WithHeadings, WithTitle, WithEvents, WithStyles, ShouldAutoSize
{
    public function collection()
    {
        return new Collection([]);
    }

    public function headings(): array
    {
        return [
            'No',
            'Nama Paket',
            'Status Pekerjaan',
            'Nama Penyedia',
            'Status Penyedia',
            'Keterangan',
        ];
    }

    public function title(): string
    {
        return 'Validasi Import';
    }

    public function registerEvents(): array
    {
        return [
            AfterSheet::class => function (AfterSheet $event) {
                $sheet = $event->sheet->getDelegate();
                $maxRows = 500;

                for ($row = 2; $row <= $maxRows + 1; $row++) {
                    $inputRow = $row;

                    $sheet->setCellValue("A{$row}", "=ROW()-1");
                    $sheet->setCellValue("B{$row}", "='Import Kontrak'!A{$inputRow}");
                    $sheet->setCellValue("C{$row}", "=IF(B{$row}=\"\",\"\",IF(COUNTIF('Referensi Pekerjaan'!\$B:\$B,LOWER(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(TRIM('Import Kontrak'!A{$inputRow}),\" \",\"\"),\".\",\"\"),\"-\",\"\") ,\"/\",\"\") ,\",\",\"\") ,CHAR(160),\"\")))>0,\"OK\",\"Tidak ditemukan\"))");
                    $sheet->setCellValue("D{$row}", "='Import Kontrak'!B{$inputRow}");
                    $sheet->setCellValue("E{$row}", "=IF(D{$row}=\"\",\"\",IF(COUNTIF('Referensi Penyedia'!\$B:\$B,LOWER(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(TRIM('Import Kontrak'!B{$inputRow}),\" \",\"\"),\".\",\"\"),\"-\",\"\") ,\"/\",\"\") ,\",\",\"\") ,CHAR(160),\"\")))>0,\"OK\",\"Tidak ditemukan\"))");
                    $sheet->setCellValue("F{$row}", "=TRIM(IF(C{$row}<>\"OK\",\"Pekerjaan tidak ditemukan; \",\"\")&IF(E{$row}<>\"OK\",\"Penyedia tidak ditemukan\",\"\"))");
                }

                $sheet->freezePane('A2');
                $sheet->setAutoFilter("A1:F" . ($maxRows + 1));
            },
        ];
    }

    public function styles(Worksheet $sheet)
    {
        return [
            1 => ['font' => ['bold' => true]],
        ];
    }
}

class PekerjaanRefSheet implements FromCollection, WithHeadings, WithTitle, WithStyles, ShouldAutoSize
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

        return $query->get()->map(function (Pekerjaan $pekerjaan) {
            return [
                'nama_paket' => $pekerjaan->nama_paket,
                'nama_paket_normalized' => $this->normalizeLookupText($pekerjaan->nama_paket),
                'kode_rekening' => $pekerjaan->kode_rekening,
                'pagu' => $pekerjaan->pagu,
            ];
        });
    }

    public function headings(): array
    {
        return ['Nama Paket', 'Nama Paket Normalized', 'Kode Rekening', 'Pagu'];
    }

    public function title(): string
    {
        return 'Referensi Pekerjaan';
    }

    public function styles(Worksheet $sheet)
    {
        return [
            1 => ['font' => ['bold' => true]],
            'B:B' => ['font' => ['color' => ['rgb' => '666666']]],
        ];
    }

    protected function normalizeLookupText(?string $value): string
    {
        $value = preg_replace('/[^\pL\pN]+/u', '', trim((string) $value));

        return mb_strtolower($value);
    }
}

class PenyediaRefSheet implements FromCollection, WithHeadings, WithTitle, WithStyles, ShouldAutoSize
{
    public function collection()
    {
        return Penyedia::select('nama', 'direktur', 'alamat')->get()->map(function (Penyedia $penyedia) {
            return [
                'nama' => $penyedia->nama,
                'nama_normalized' => $this->normalizeLookupText($penyedia->nama),
                'direktur' => $penyedia->direktur,
                'direktur_normalized' => $this->normalizeLookupText($penyedia->direktur),
                'alamat' => $penyedia->alamat,
            ];
        });
    }

    public function headings(): array
    {
        return ['Nama Penyedia', 'Nama Normalized', 'Direktur', 'Direktur Normalized', 'Alamat'];
    }

    public function title(): string
    {
        return 'Referensi Penyedia';
    }

    public function styles(Worksheet $sheet)
    {
        return [
            1 => ['font' => ['bold' => true]],
            'B:B' => ['font' => ['color' => ['rgb' => '666666']]],
            'D:D' => ['font' => ['color' => ['rgb' => '666666']]],
        ];
    }

    protected function normalizeLookupText(?string $value): string
    {
        $value = preg_replace('/[^\pL\pN]+/u', '', trim((string) $value));

        return mb_strtolower($value);
    }
}
