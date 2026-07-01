<?php

namespace App\Services;

use Illuminate\Support\Str;
use PhpOffice\PhpSpreadsheet\Cell\Coordinate;
use PhpOffice\PhpSpreadsheet\IOFactory;

class ExcelDocumentExportService
{
    public function __construct(
        private readonly KontrakDocumentDataBuilder $dataBuilder,
    ) {}

    public function exportRingkasan($kontrak, array $overrideData = []): string
    {
        $templatePath = KontrakTemplateService::resolvePath('kontrak_template_ringkasan');
        $extension = strtolower(pathinfo($templatePath, PATHINFO_EXTENSION));

        if ($extension !== 'xlsx') {
            throw new \Exception('Template ringkasan kontrak harus berformat .xlsx.');
        }

        $pekerjaan = $kontrak->pekerjaans->first();
        if (! $pekerjaan) {
            throw new \Exception('Kontrak tidak memiliki pekerjaan terkait.');
        }

        $data = $this->dataBuilder->build($pekerjaan, $kontrak, $overrideData);

        $spreadsheet = IOFactory::load($templatePath);

        foreach ($spreadsheet->getAllSheets() as $sheet) {
            $highestRow = $sheet->getHighestRow();
            $highestColumnIndex = Coordinate::columnIndexFromString($sheet->getHighestColumn());

            for ($row = 1; $row <= $highestRow; $row++) {
                for ($columnIndex = 1; $columnIndex <= $highestColumnIndex; $columnIndex++) {
                    $cellAddress = Coordinate::stringFromColumnIndex($columnIndex).$row;
                    $cell = $sheet->getCell($cellAddress);
                    $value = $cell->getValue();

                    if (! is_string($value) || $value === '') {
                        continue;
                    }

                    $replaced = $this->replacePlaceholders($value, $data);
                    if ($replaced !== $value) {
                        $cell->setValue($replaced);
                    }
                }
            }
        }

        $fileName = 'Ringkasan_'.Str::slug($pekerjaan->nama_paket).'_'.date('YmdHis').'.xlsx';
        $tempPath = storage_path('app/public/temp/'.$fileName);

        if (! is_dir(storage_path('app/public/temp'))) {
            mkdir(storage_path('app/public/temp'), 0755, true);
        }

        $writer = IOFactory::createWriter($spreadsheet, 'Xlsx');
        $writer->save($tempPath);

        return $tempPath;
    }

    private function replacePlaceholders(string $value, array $data): string
    {
        foreach ($data as $key => $replacement) {
            $val = $replacement === null || $replacement === '' ? '-' : (string) $replacement;
            $value = preg_replace('/\{\{\s*'.preg_quote((string) $key, '/').'\s*\}\}/', $val, $value);
            $value = preg_replace('/\{\s*'.preg_quote((string) $key, '/').'\s*\}/', $val, $value);
        }

        return $value;
    }
}