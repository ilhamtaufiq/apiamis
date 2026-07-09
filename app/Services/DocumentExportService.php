<?php

namespace App\Services;

use Illuminate\Support\Str;
use PhpOffice\PhpWord\TemplateProcessor;

class DocumentExportService
{
    public function __construct(
        private readonly KontrakDocumentDataBuilder $dataBuilder,
        private readonly ExcelDocumentExportService $excelExportService,
    ) {}

    /**
     * Export Kontrak to Word/PDF
     */
    public function export($kontrak, $format = 'docx', $templateKey = 'kontrak_template_spk', $overrideData = [])
    {
        $templatePath = KontrakTemplateService::resolvePath($templateKey);

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
        $data = $this->dataBuilder->build($kontrak->pekerjaans->first(), $kontrak, $overrideData);

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
                        $val = $value === null || $value === '' ? '-' : (string) $value;
                        // Support both {key} and {{key}}
                        $xml = preg_replace('/\{\{\s*'.preg_quote($key, '/').'\s*\}\}/', $val, $xml);
                        $xml = preg_replace('/\{\s*'.preg_quote($key, '/').'\s*\}/', $val, $xml);
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
        $fileName = 'Kontrak_'.Str::slug($kontrak->pekerjaans->first()?->nama_paket).'_'.date('YmdHis').'.docx';
        $tempPath = storage_path('app/public/temp/'.$fileName);

        if (! is_dir(storage_path('app/public/temp'))) {
            mkdir(storage_path('app/public/temp'), 0755, true);
        }

        $templateProcessor->saveAs($tempPath);

        if ($format === 'pdf') {
            try {
                \PhpOffice\PhpWord\Settings::setPdfRendererName(\PhpOffice\PhpWord\Settings::PDF_RENDERER_DOMPDF);
                \PhpOffice\PhpWord\Settings::setPdfRendererPath(base_path('vendor/dompdf/dompdf'));

                $phpWord = \PhpOffice\PhpWord\IOFactory::load($tempPath);

                $pdfFileName = 'Kontrak_'.Str::slug($kontrak->pekerjaan->nama_paket).'_'.date('YmdHis').'.pdf';
                $pdfPath = storage_path('app/public/temp/'.$pdfFileName);

                $pdfWriter = \PhpOffice\PhpWord\IOFactory::createWriter($phpWord, 'PDF');
                $pdfWriter->save($pdfPath);

                if (file_exists($tempPath)) {
                    unlink($tempPath);
                }

                return $pdfPath;
            } catch (\Exception $e) {
                throw new \Exception('Gagal konversi ke PDF: '.$e->getMessage());
            }
        }

        return $tempPath;
    }

    public function exportRingkasan($kontrak, $format = 'xlsx', array $overrideData = [])
    {
        return $this->excelExportService->exportRingkasan($kontrak, $overrideData);
    }

    public function exportBAP($kontrak, $format = 'docx', $overrideData = [])
    {
        return $this->export($kontrak, $format, 'kontrak_template_bap', $overrideData);
    }

    public function exportCover($kontrak, $format = 'docx')
    {
        $subBidang = strtolower((string) (
            $kontrak->pekerjaans->first()?->kegiatan?->sub_bidang
            ?? $kontrak->kegiatan?->sub_bidang
            ?? ''
        ));

        if (str_contains($subBidang, 'air minum')) {
            return $this->export($kontrak, $format, 'kontrak_template_cover_am');
        }

        if (str_contains($subBidang, 'sanitasi')) {
            return $this->export($kontrak, $format, 'kontrak_template_cover_san');
        }

        throw new \Exception('Template cover kontrak untuk sub bidang ini belum tersedia.');
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
        return preg_replace_callback('/\{[^{}]*\}/U', function ($match) {
            return strip_tags($match[0]);
        }, $xml);
    }

}
