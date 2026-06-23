<?php

namespace App\Services;

use Dompdf\Dompdf;
use Illuminate\Support\Str;
use PhpOffice\PhpSpreadsheet\IOFactory as SpreadsheetIOFactory;
use PhpOffice\PhpSpreadsheet\Writer\Pdf\Dompdf as SpreadsheetPdfWriter;
use Spatie\MediaLibrary\MediaCollections\Models\Media;

class DocumentPdfConverter
{
    private const IMAGE_EXTENSIONS = ['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp'];

    private const SPREADSHEET_EXTENSIONS = ['xlsx', 'xls', 'ods', 'csv'];

    public function convertMediaToPdf(Media $media): ?string
    {
        $inputPath = $media->getPath();

        if (! file_exists($inputPath)) {
            return null;
        }

        $extension = strtolower(pathinfo($media->file_name, PATHINFO_EXTENSION));

        if ($extension === 'pdf') {
            return $inputPath;
        }

        $outputDir = storage_path('app/temp-pdf');

        if (! file_exists($outputDir)) {
            mkdir($outputDir, 0775, true);
        }

        $outputPath = $outputDir.'/'.Str::uuid().'.pdf';

        if (in_array($extension, self::IMAGE_EXTENSIONS, true)) {
            return $this->convertImageToPdf($inputPath, $outputPath) ? $outputPath : null;
        }

        if (in_array($extension, self::SPREADSHEET_EXTENSIONS, true)) {
            return $this->convertSpreadsheetToPdf($inputPath, $outputPath) ? $outputPath : null;
        }

        return $this->convertWithLibreOffice($inputPath, $outputPath, $media->file_name);
    }

    public function getSuggestedDownloadName(Media $media): string
    {
        return pathinfo($media->file_name, PATHINFO_FILENAME).'.pdf';
    }

    private function convertImageToPdf(string $inputPath, string $outputPath): bool
    {
        try {
            $imageData = base64_encode((string) file_get_contents($inputPath));
            $mimeType = mime_content_type($inputPath) ?: 'image/jpeg';

            $html = <<<HTML
            <!DOCTYPE html>
            <html>
            <head>
                <style>
                    @page { margin: 0; }
                    body { margin: 0; padding: 0; }
                    img { width: 100%; height: auto; display: block; }
                </style>
            </head>
            <body>
                <img src="data:{$mimeType};base64,{$imageData}" alt="document" />
            </body>
            </html>
            HTML;

            $pdf = new Dompdf();
            $pdf->loadHtml($html);
            $pdf->setPaper('A4', 'portrait');
            $pdf->render();
            file_put_contents($outputPath, $pdf->output());

            return file_exists($outputPath);
        } catch (\Throwable) {
            return false;
        }
    }

    private function convertSpreadsheetToPdf(string $inputPath, string $outputPath): bool
    {
        try {
            $spreadsheet = SpreadsheetIOFactory::load($inputPath);
            $writer = new SpreadsheetPdfWriter($spreadsheet);
            $writer->save($outputPath);

            return file_exists($outputPath);
        } catch (\Throwable) {
            return false;
        }
    }

    private function convertWithLibreOffice(string $inputPath, string $outputPath, string $originalFileName): ?string
    {
        $libreOffice = $this->resolveLibreOfficeBinary();

        if ($libreOffice === null) {
            return null;
        }

        $outputDir = dirname($outputPath);
        $profileDir = storage_path('app/libreoffice-profile');

        if (! file_exists($profileDir)) {
            mkdir($profileDir, 0775, true);
        }

        $profileUri = $this->pathToFileUri($profileDir);
        $command = $this->buildLibreOfficeCommand(
            $libreOffice,
            $profileUri,
            $outputDir,
            $inputPath,
        );

        exec($command, $output, $returnVar);

        if ($returnVar !== 0) {
            return null;
        }

        $convertedName = pathinfo($originalFileName, PATHINFO_FILENAME).'.pdf';
        $convertedPath = $outputDir.DIRECTORY_SEPARATOR.$convertedName;

        if (! file_exists($convertedPath)) {
            return null;
        }

        if ($convertedPath !== $outputPath) {
            rename($convertedPath, $outputPath);
        }

        return file_exists($outputPath) ? $outputPath : null;
    }

    private function resolveLibreOfficeBinary(): ?string
    {
        $configured = env('LIBREOFFICE_PATH');

        if (is_string($configured) && $configured !== '' && $this->isExecutable($configured)) {
            return $configured;
        }

        $candidates = PHP_OS_FAMILY === 'Windows'
            ? [
                'C:\\Program Files\\LibreOffice\\program\\soffice.exe',
                'C:\\Program Files (x86)\\LibreOffice\\program\\soffice.exe',
            ]
            : ['libreoffice', 'soffice', '/usr/bin/libreoffice', '/usr/bin/soffice'];

        foreach ($candidates as $candidate) {
            if ($this->isExecutable($candidate)) {
                return $candidate;
            }
        }

        return null;
    }

    private function isExecutable(string $path): bool
    {
        if (PHP_OS_FAMILY === 'Windows') {
            return file_exists($path);
        }

        if (str_contains($path, DIRECTORY_SEPARATOR)) {
            return is_executable($path);
        }

        $command = sprintf('command -v %s 2>/dev/null', escapeshellarg($path));
        exec($command, $output, $returnVar);

        return $returnVar === 0 && ! empty($output[0]);
    }

    private function buildLibreOfficeCommand(
        string $libreOffice,
        string $profileUri,
        string $outputDir,
        string $inputPath,
    ): string {
        $binary = escapeshellarg($libreOffice);
        $outdir = escapeshellarg($outputDir);
        $input = escapeshellarg($inputPath);
        $profile = '-env:UserInstallation='.$profileUri;

        if (PHP_OS_FAMILY === 'Windows') {
            $tempHome = storage_path('app/libreoffice-home');

            return 'set "HOME='.$tempHome.'" && '.$binary.' --headless '.$profile.' --convert-to pdf --outdir '.$outdir.' '.$input;
        }

        $tempHome = '/tmp';

        return "HOME={$tempHome} {$binary} --headless {$profile} --convert-to pdf --outdir {$outdir} {$input}";
    }

    private function pathToFileUri(string $path): string
    {
        $normalized = str_replace('\\', '/', $path);

        if (PHP_OS_FAMILY === 'Windows' && preg_match('/^[A-Za-z]:/', $normalized) === 1) {
            return 'file:///'.str_replace(' ', '%20', $normalized);
        }

        return 'file://'.$normalized;
    }
}