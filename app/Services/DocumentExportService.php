<?php

namespace App\Services;

use PhpOffice\PhpWord\TemplateProcessor;
use Illuminate\Support\Str;
use Carbon\Carbon;
use Illuminate\Support\Facades\Storage;

class DocumentExportService
{
    /**
     * Export Kontrak to Word/PDF
     */
    public function export($kontrak, $format = 'docx', $templateName = 'SPK_Template.docx', $overrideData = [])
    {
        $templatePath = storage_path('app/templates/' . $templateName);
        
        if (!file_exists($templatePath)) {
            throw new \Exception("Template tidak ditemukan di: " . $templatePath);
        }

        $templateProcessor = new TemplateProcessor($templatePath);
        
        // 1. Clean the XML to join fragmented placeholders like { n_o_m_o_r }
        $this->cleanTemplateXml($templateProcessor);

        // 2. Force delimiters to { } instead of default ${ }
        $reflection = new \ReflectionClass($templateProcessor);
        $propertyOpen = $reflection->getProperty('macroOpeningChars');
        $propertyOpen->setAccessible(true);
        $propertyOpen->setValue($templateProcessor, '{');
        $propertyClose = $reflection->getProperty('macroClosingChars');
        $propertyClose->setAccessible(true);
        $propertyClose->setValue($templateProcessor, '}');

        // 3. Prepare Data
        $data = $this->prepareData($kontrak->pekerjaan, $kontrak, $overrideData);

        // 4. Direct XML Replacement (More reliable than TemplateProcessor for custom braces)
        // We replace in Main Part, Headers, and Footers
        $parts = ['tempDocumentMainPart', 'tempDocumentHeaders', 'tempDocumentFooters'];
        
        foreach ($parts as $partName) {
            try {
                $property = $reflection->getProperty($partName);
                $property->setAccessible(true);
                $partXmls = $property->getValue($templateProcessor);
                
                // Headers and Footers are arrays, Main Part is a string
                $isSingle = is_string($partXmls);
                $xmlArray = $isSingle ? [$partXmls] : $partXmls;
                
                foreach ($xmlArray as $idx => $xml) {
                    foreach ($data as $key => $value) {
                        $val = (string)$value;
                        // Support both {key} and {{key}}
                        $xml = preg_replace('/\{\{\s*' . preg_quote($key, '/') . '\s*\}\}/', $val, $xml);
                        $xml = preg_replace('/\{\s*' . preg_quote($key, '/') . '\s*\}/', $val, $xml);
                    }
                    $xmlArray[$idx] = $xml;
                }
                
                $property->setValue($templateProcessor, $isSingle ? $xmlArray[0] : $xmlArray);
            } catch (\ReflectionException $e) {
                // Property might not exist in some versions or if not used
                continue;
            }
        }

        // 5. Save to temporary file
        $fileName = 'Kontrak_' . Str::slug($kontrak->pekerjaan->nama_paket) . '_' . date('YmdHis') . '.docx';
        $tempPath = storage_path('app/public/temp/' . $fileName);
        
        if (!is_dir(storage_path('app/public/temp'))) {
            mkdir(storage_path('app/public/temp'), 0755, true);
        }

        $templateProcessor->saveAs($tempPath);

        if ($format === 'pdf') {
            try {
                \PhpOffice\PhpWord\Settings::setPdfRendererName(\PhpOffice\PhpWord\Settings::PDF_RENDERER_DOMPDF);
                \PhpOffice\PhpWord\Settings::setPdfRendererPath(base_path('vendor/dompdf/dompdf'));
                
                $phpWord = \PhpOffice\PhpWord\IOFactory::load($tempPath);
                
                $pdfFileName = 'Kontrak_' . Str::slug($kontrak->pekerjaan->nama_paket) . '_' . date('YmdHis') . '.pdf';
                $pdfPath = storage_path('app/public/temp/' . $pdfFileName);
                
                $pdfWriter = \PhpOffice\PhpWord\IOFactory::createWriter($phpWord, 'PDF');
                $pdfWriter->save($pdfPath);
                
                if (file_exists($tempPath)) {
                    unlink($tempPath);
                }
                
                return $pdfPath;
            } catch (\Exception $e) {
                throw new \Exception("Gagal konversi ke PDF: " . $e->getMessage());
            }
        }

        return $tempPath;
    }

    public function exportRingkasan($kontrak, $format = 'docx')
    {
        return $this->export($kontrak, $format, 'ringkasan_kontrak_template.docx');
    }

    public function exportBAP($kontrak, $format = 'docx', $overrideData = [])
    {
        return $this->export($kontrak, $format, 'bap_template.docx', $overrideData);
    }

    private function prepareData($pekerjaan, $kontrak, $overrideData = [])
    {
        // Set Locale for Carbon and System
        Carbon::setLocale('id');
        setlocale(LC_ALL, 'id_ID', 'id_ID.UTF-8', 'Indonesian');

        $kegiatan = $pekerjaan->kegiatan;
        $penyedia = $kontrak->penyedia;

        $data = [
            // Pekerjaan details
            'nama_paket' => $pekerjaan->nama_paket,
            'pagu' => 'Rp. ' . number_format($pekerjaan->pagu, 0, ',', '.'),
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
            'nilai_kontrak' => 'Rp. ' . number_format($kontrak->nilai_kontrak, 0, ',', '.'),
            'nilai_kontrak_terbilang' => $this->terbilang($kontrak->nilai_kontrak),
            'tgl_sppbj' => $kontrak->tgl_sppbj instanceof Carbon ? $kontrak->tgl_sppbj->translatedFormat('d F Y') : '-',
            'tgl_spk' => $kontrak->tgl_spk instanceof Carbon ? $kontrak->tgl_spk->translatedFormat('d F Y') : '-',
            'tgl_selesai' => $kontrak->tgl_selesai instanceof Carbon ? $kontrak->tgl_selesai->translatedFormat('d F Y') : '-',
            'tgl_spmk' => $kontrak->tgl_spmk instanceof Carbon ? $kontrak->tgl_spmk->translatedFormat('d F Y') : '-',
            'nomor_sppbj' => $kontrak->sppbj ?: '-',
            'nomor_spk' => $kontrak->spk ?: '-',
            'nomor_spmk' => $kontrak->spmk ?: '-',
            'kode_rup' => $kontrak->kode_rup ?: '-',
            'kode_paket' => $kontrak->kode_paket ?: '-',
            'nomor_penawaran' => $kontrak->nomor_penawaran ?: '-',
            'tanggal_penawaran' => $kontrak->tanggal_penawaran instanceof Carbon ? $kontrak->tanggal_penawaran->translatedFormat('d F Y') : '-',

            // Penyedia
            'nama_penyedia' => $penyedia ? $penyedia->nama : '-',
            'direktur' => $penyedia ? $penyedia->direktur : '-',
            'nama_direktur' => $penyedia ? $penyedia->direktur : '-',
            'alamat_penyedia' => $penyedia ? $penyedia->alamat : '-',
            'bank' => $penyedia ? $penyedia->bank : '-',
            'norek' => $penyedia ? $penyedia->norek : '-',
            'no_akta' => $penyedia ? $penyedia->no_akta : '-',
            'notaris' => $penyedia ? $penyedia->notaris : '-',
            'tanggal_akta' => $penyedia && $penyedia->tanggal_akta instanceof Carbon ? $penyedia->tanggal_akta->translatedFormat('d F Y') : '-',
            
            // Waktu & Masa Pelaksanaan
            'masa_hari' => ($kontrak->tgl_spmk instanceof Carbon && $kontrak->tgl_selesai instanceof Carbon)
                ? (int) $kontrak->tgl_spmk->diffInDays($kontrak->tgl_selesai) + 1 
                : '-',
            'masa_hari_terbilang' => ($kontrak->tgl_spmk instanceof Carbon && $kontrak->tgl_selesai instanceof Carbon)
                ? $this->terbilang((int) $kontrak->tgl_spmk->diffInDays($kontrak->tgl_selesai) + 1) 
                : '-',
        ];

        // Additional Mappings for consistency
        $data['Pekerjaan'] = $data['nama_paket'];
        $data['Penyedia'] = $data['nama_penyedia'];
        $data['Nilai_Kontrak'] = $data['nilai_kontrak'];
        $data['Terbilang'] = $data['nilai_kontrak_terbilang'];
        $data['Kota'] = $pekerjaan->kecamatan ? $pekerjaan->kecamatan->nama : 'Cianjur';
        $data['SPK'] = $data['tgl_spk'];
        $data['SPK1'] = $data['nomor_spk'];
        $data['SPPBJ'] = $data['tgl_sppbj'];
        $data['SPPBJ1'] = $data['nomor_sppbj'];
        $data['Masa'] = $data['masa_hari'];
        $data['Selesai'] = $data['tgl_selesai'];
        $data['tgl_spl'] = $data['tgl_spk'];

        // Merge override data (for BAP calculations)
        if (!empty($overrideData)) {
            foreach ($overrideData as $key => $value) {
                $lowerKey = strtolower($key);
                
                // Fields that MUST be formatted as money
                $moneyKeywords = ['nilai', 'jumlah', 'dpp', 'ppn', 'total', 'tagihan'];
                $isMoney = false;
                foreach ($moneyKeywords as $mk) {
                    if (str_contains($lowerKey, $mk)) {
                        $isMoney = true;
                        break;
                    }
                }

                // Fields that should NOT be formatted as money (unless they are in the money list above)
                $excludeKeywords = ['nomor', 'tgl', 'tanggal', 'tahun', 'kode', 'id', 'rate', 'hari'];
                $isExcluded = false;
                if (!$isMoney) {
                    foreach ($excludeKeywords as $kw) {
                        if (str_contains($lowerKey, $kw)) {
                            $isExcluded = true;
                            break;
                        }
                    }
                }
                
                // Special case: if it's numeric and (isMoney OR not excluded), format it
                if (is_numeric($value)) {
                    if ($isMoney || !$isExcluded) {
                        $data[$key] = 'Rp. ' . number_format((float)$value, 0, ',', '.');
                    } else {
                        $data[$key] = $value;
                    }
                } else {
                    $data[$key] = $value;
                }
            }
        }

        return $data;
    }

    /**
     * Clean fragmented XML tags between { and }
     */
    private function cleanTemplateXml($templateProcessor)
    {
        $reflection = new \ReflectionClass($templateProcessor);
        $parts = ['tempDocumentMainPart', 'tempDocumentHeaders', 'tempDocumentFooters'];
        
        foreach ($parts as $partName) {
            try {
                $property = $reflection->getProperty($partName);
                $property->setAccessible(true);
                $partXmls = $property->getValue($templateProcessor);
                
                if (is_string($partXmls)) {
                    $property->setValue($templateProcessor, $this->joinFragmentedTags($partXmls));
                } elseif (is_array($partXmls)) {
                    foreach ($partXmls as $idx => $xml) {
                        $partXmls[$idx] = $this->joinFragmentedTags($xml);
                    }
                    $property->setValue($templateProcessor, $partXmls);
                }
            } catch (\ReflectionException $e) {
                continue;
            }
        }
    }

    private function joinFragmentedTags($xml)
    {
        // Join tags like {<...><...>} into {}
        return preg_replace_callback('/\{[^{}]*\}/U', function($match) {
            return strip_tags($match[0]);
        }, $xml);
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
