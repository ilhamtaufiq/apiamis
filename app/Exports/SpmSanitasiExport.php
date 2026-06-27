<?php

namespace App\Exports;

use App\Models\SpmSanitasi;
use Maatwebsite\Excel\Concerns\Exportable;
use Maatwebsite\Excel\Concerns\WithMultipleSheets;

class SpmSanitasiExport implements WithMultipleSheets
{
    use Exportable;

    public function __construct(
        protected ?int $kecamatanId = null,
        protected ?int $desaId = null,
        protected ?string $search = null,
    ) {
    }

    public function sheets(): array
    {
        return [
            new SpmSanitasiSheetExport('spaldt', 'SPALDT', 'FORMAT DATA SPALDT', $this->kecamatanId, $this->desaId, $this->search),
            new SpmSanitasiSheetExport('spalds', 'SPALDS', 'FORMAT DATA SPALDS', $this->kecamatanId, $this->desaId, $this->search),
            new SpmSanitasiSheetExport('iplt', 'IPLT', 'FORMAT DATA IPLT', $this->kecamatanId, $this->desaId, $this->search),
        ];
    }
}

class SpmSanitasiSheetExport implements \Maatwebsite\Excel\Concerns\FromCollection, \Maatwebsite\Excel\Concerns\WithTitle, \Maatwebsite\Excel\Concerns\WithEvents
{
    use Exportable;

    public function __construct(
        protected string $jenis,
        protected string $title,
        protected string $formatTitle,
        protected ?int $kecamatanId,
        protected ?int $desaId,
        protected ?string $search,
    ) {
    }

    public function collection()
    {
        return collect([]);
    }

    public function title(): string
    {
        return $this->title;
    }

    public function registerEvents(): array
    {
        return [
            \Maatwebsite\Excel\Events\AfterSheet::class => function (\Maatwebsite\Excel\Events\AfterSheet $event) {
                $sheet = $event->sheet->getDelegate();
                $sheet->setCellValue('A1', $this->formatTitle);

                $headers = $this->jenis === 'iplt'
                    ? $this->ipltHeaders()
                    : $this->spaldHeaders($this->jenis === 'spaldt');

                $sheet->fromArray($headers, null, 'A4');

                $rows = $this->queryRows();
                $rowIndex = 5;
                $no = 1;

                foreach ($rows as $item) {
                    $sheet->fromArray($this->mapItem($item, $no), null, 'A'.$rowIndex);
                    $rowIndex++;
                    $no++;
                }
            },
        ];
    }

    private function queryRows()
    {
        $query = SpmSanitasi::with('desa.kecamatan')->where('jenis', $this->jenis);

        if ($this->kecamatanId) {
            $query->whereHas('desa', fn ($q) => $q->where('kecamatan_id', $this->kecamatanId));
        }

        if ($this->desaId) {
            $query->where('desa_id', $this->desaId);
        }

        if ($this->search) {
            $search = '%'.$this->search.'%';
            $query->where(function ($q) use ($search) {
                $q->where('nama_infrastruktur', 'like', $search)
                    ->orWhere('alamat_lengkap', 'like', $search)
                    ->orWhereHas('desa', fn ($dq) => $dq->where('n_desa', 'like', $search));
            });
        }

        return $query->orderBy('id')->get();
    }

    private function spaldHeaders(bool $withJiwa): array
    {
        $headers = [
            'No.',
            'Skala Pelayanan',
            'Provinsi',
            'Kabupaten',
            'Kecamatan',
            'Desa/Kelurahan',
            'Nama Infrastruktur',
            'Latitude',
            'Longitude',
            'Alamat Lengkap',
            'Jumlah Pemanfaat (KK)',
        ];

        if ($withJiwa) {
            $headers[] = 'Jumlah Pemanfaat (Jiwa)';
        }

        return array_merge($headers, [
            'Tahun Konstruksi',
            "Data Pembiayaan\n(APBN)",
            "Data Pembiayaan\n(APBD)",
            "Data Pembiayaan\n(DAK)",
            "Data Pembiayaan\n(Hibah)",
            "Data Pembiayaan\n(CSR)",
            "Data Pembiayaan\n(Lain-Lain)",
            "Data Pembiayaan\n(TOTAL)",
            'Status Keberfungsian',
            'Kualitas Keberfungsian',
            'Pengelola',
            'Kapasitas Desain terpasang (m3/hari)',
            'Kapasitas Terpakai (m3/hari)',
            'Kapasitas Tidak Terpakai (m3/hari)',
            'Jenis Pengolahan',
            'Peta Cakupan Air Limbah',
            'Status Lahan',
            'Luas Lahan (ha)',
            'Opsi Teknologi',
            'Jumlah Stasiun Pompa (unit)',
            'Biaya Operasional (Rp)',
        ]);
    }

    private function ipltHeaders(): array
    {
        return [
            'No.',
            'Provinsi',
            'Kabupaten',
            'Kecamatan',
            'Desa/Kelurahan',
            'Nama IPLT',
            'Latitude',
            'Longitude',
            'Tahun Konstruksi',
            'Jenis Pengelola',
            "Kapasitas Terpasang\n(m3/hari)",
            "Kapasitas Terpakai\n(m3/hari)",
            "Kapasitas Tidak Terpakai\n(m3/hari)",
            'Status Keberfungsian',
            'Kualitas Keberfungsian',
            'Sistem Pengolahan',
            "Truk Tinja\n(unit)",
            "Kapasitas Truk\n(m3)",
            "Jumlah Ritasi\n(rit/hari)",
            "Jarak maksimal Pelayanan\n(km)",
            "Alokasi Biaya Operasional\n(Rp/tahun)",
            "Jumlah Pemanfaat\n(KK)",
            'Tahun Konstruksi',
            "Data Pembiayaan\n(APBN)",
            "Data Pembiayaan\n(APBD)",
            "Data Pembiayaan\n(DAK)",
            "Data Pembiayaan\n(Hibah)",
            "Data Pembiayaan\n(CSR)",
            "Data Pembiayaan\n(Lain-Lain)",
            "Data Pembiayaan\n(TOTAL)",
        ];
    }

    private function mapItem(SpmSanitasi $item, int $no): array
    {
        $kec = $item->desa?->kecamatan?->n_kec ?? '';
        $desa = $item->desa?->n_desa ?? '';

        if ($this->jenis === 'iplt') {
            return [
                $no,
                'Jawa Barat',
                'Cianjur',
                $kec,
                $desa,
                $item->nama_infrastruktur,
                $item->latitude,
                $item->longitude,
                $item->tahun_konstruksi,
                $item->jenis_pengelola,
                $item->kapasitas_desain,
                $item->kapasitas_terpakai,
                $item->kapasitas_tidak_terpakai,
                $item->status_keberfungsian,
                $item->kualitas_keberfungsian,
                $item->sistem_pengolahan,
                $item->truk_tinja_unit,
                $item->kapasitas_truk_m3,
                $item->jumlah_ritasi,
                $item->jarak_maksimal_pelayanan_km,
                $item->alokasi_biaya_operasional,
                $item->jumlah_pemanfaat_kk,
                $item->tahun_konstruksi,
                $item->pembiayaan_apbn,
                $item->pembiayaan_apbd,
                $item->pembiayaan_dak,
                $item->pembiayaan_hibah,
                $item->pembiayaan_csr,
                $item->pembiayaan_lain,
                $item->pembiayaan_total,
            ];
        }

        $row = [
            $no,
            $item->skala_pelayanan,
            'Jawa Barat',
            'Cianjur',
            $kec,
            $desa,
            $item->nama_infrastruktur,
            $item->latitude,
            $item->longitude,
            $item->alamat_lengkap,
            $item->jumlah_pemanfaat_kk,
        ];

        if ($this->jenis === 'spaldt') {
            $row[] = $item->jumlah_pemanfaat_jiwa;
        }

        return array_merge($row, [
            $item->tahun_konstruksi,
            $item->pembiayaan_apbn,
            $item->pembiayaan_apbd,
            $item->pembiayaan_dak,
            $item->pembiayaan_hibah,
            $item->pembiayaan_csr,
            $item->pembiayaan_lain,
            $item->pembiayaan_total,
            $item->status_keberfungsian,
            $item->kualitas_keberfungsian,
            $item->pengelola,
            $item->kapasitas_desain,
            $item->kapasitas_terpakai,
            $item->kapasitas_tidak_terpakai,
            $item->jenis_pengolahan,
            $item->peta_cakupan,
            $item->status_lahan,
            $item->luas_lahan_ha,
            $item->opsi_teknologi,
            $item->jumlah_stasiun_pompa,
            $item->biaya_operasional,
        ]);
    }
}