<?php

namespace App\Services;

use App\Models\Pekerjaan;
use App\Models\Kontrak;
use PhpOffice\PhpWord\TemplateProcessor;
use Illuminate\Support\Str;
use Carbon\Carbon;

class DocumentExportService
{
    /**
     * Export Kontrak to Word
     */
    public function exportKontrak(Pekerjaan $pekerjaan, string $templatePath = null)
    {
        $templatePath = $templatePath ?: base_path('Template_Kontrak.docx');

        if (!file_exists($templatePath)) {
            throw new \Exception("Template not found at: {$templatePath}");
        }

        $templateProcessor = new TemplateProcessor($templatePath);

        // Get Main Contract (latest or first)
        $kontrak = $pekerjaan->kontrak()->latest()->first();
        $penyedia = $kontrak ? $kontrak->penyedia : null;
        $kegiatan = $pekerjaan->kegiatan;
        
        // Mappings
        $data = [
            'nama_paket' => $pekerjaan->nama_paket,
            'pagu' => number_format($pekerjaan->pagu, 0, ',', '.'),
            'pagu_terbilang' => $this->terbilang($pekerjaan->pagu),
            'kode_rekening' => $pekerjaan->kode_rekening,
            'kecamatan' => $pekerjaan->kecamatan ? $pekerjaan->kecamatan->nama : '-',
            'desa' => $pekerjaan->desa ? $pekerjaan->desa->nama : '-',
            
            // Kegiatan
            'nama_program' => $kegiatan ? $kegiatan->nama_program : '-',
            'nama_kegiatan' => $kegiatan ? $kegiatan->nama_kegiatan : '-',
            'sub_kegiatan' => $kegiatan ? $kegiatan->nama_sub_kegiatan : '-',
            'tahun' => $kegiatan ? $kegiatan->tahun_anggaran : '-',

            // Kontrak details
            'nilai_kontrak' => $kontrak ? number_format($kontrak->nilai_kontrak, 0, ',', '.') : '-',
            'nilai_kontrak_terbilang' => $kontrak ? $this->terbilang($kontrak->nilai_kontrak) : '-',
            'tgl_sppbj' => $kontrak && $kontrak->tgl_sppbj ? $kontrak->tgl_sppbj->translatedFormat('d F Y') : '-',
            'tgl_spk' => $kontrak && $kontrak->tgl_spk ? $kontrak->tgl_spk->translatedFormat('d F Y') : '-',
            'tgl_spmk' => $kontrak && $kontrak->tgl_spmk ? $kontrak->tgl_spmk->translatedFormat('d F Y') : '-',
            'tgl_selesai' => $kontrak && $kontrak->tgl_selesai ? $kontrak->tgl_selesai->translatedFormat('d F Y') : '-',
            'nomor_sppbj' => $kontrak ? $kontrak->sppbj : '-',
            'nomor_spk' => $kontrak ? $kontrak->spk : '-',
            'nomor_spmk' => $kontrak ? $kontrak->spmk : '-',

            // Penyedia
            'nama_penyedia' => $penyedia ? $penyedia->nama : '-',
            'direktur' => $penyedia ? $penyedia->direktur : '-',
            'alamat_penyedia' => $penyedia ? $penyedia->alamat : '-',
            'bank' => $penyedia ? $penyedia->bank : '-',
            'norek' => $penyedia ? $penyedia->norek : '-',
        ];

        // Apply to template
        foreach ($data as $key => $value) {
            $templateProcessor->setValue($key, $value);
        }

        // Save to temporary file
        $fileName = 'Kontrak_' . Str::slug($pekerjaan->nama_paket) . '.docx';
        $tempPath = storage_path('app/public/temp/' . $fileName);
        
        if (!is_dir(storage_path('app/public/temp'))) {
            mkdir(storage_path('app/public/temp'), 0755, true);
        }

        $templateProcessor->saveAs($tempPath);

        return $tempPath;
    }

    /**
     * Helper to convert number to words (Indonesian)
     */
    private function terbilang($angka)
    {
        $angka = abs($angka);
        $baca = array("", "Satu", "Dua", "Tiga", "Empat", "Lima", "Enam", "Tujuh", "Delapan", "Sembilan", "Sepuluh", "Sebelas");
        $terbilang = "";

        if ($angka < 12) {
            $terbilang = " " . $baca[$angka];
        } else if ($angka < 20) {
            $terbilang = $this->terbilang($angka - 10) . " Belas";
        } else if ($angka < 100) {
            $terbilang = $this->terbilang($angka / 10) . " Puluh" . $this->terbilang($angka % 10);
        } else if ($angka < 200) {
            $terbilang = " Seratus" . $this->terbilang($angka - 100);
        } else if ($angka < 1000) {
            $terbilang = $this->terbilang($angka / 100) . " Ratus" . $this->terbilang($angka % 100);
        } else if ($angka < 2000) {
            $terbilang = " Seribu" . $this->terbilang($angka - 1000);
        } else if ($angka < 1000000) {
            $terbilang = $this->terbilang($angka / 1000) . " Ribu" . $this->terbilang($angka % 1000);
        } else if ($angka < 1000000000) {
            $terbilang = $this->terbilang($angka / 1000000) . " Juta" . $this->terbilang($angka % 1000000);
        } else if ($angka < 1000000000000) {
            $terbilang = $this->terbilang($angka / 1000000000) . " Milyar" . $this->terbilang(fmod($angka, 1000000000));
        } else if ($angka < 1000000000000000) {
            $terbilang = $this->terbilang($angka / 1000000000000) . " Trilyun" . $this->terbilang(fmod($angka, 1000000000000));
        }

        return trim($terbilang);
    }
}
