<?php

namespace Tests\Unit;

use App\Services\Procurement\SpseDocumentDownloader;
use App\Services\Procurement\SpseHttpClient;
use PHPUnit\Framework\TestCase;

class SpseDocumentDownloaderTest extends TestCase
{
    public function test_resolves_pdf_mime_from_body(): void
    {
        $downloader = new SpseDocumentDownloader(new SpseHttpClient(new \App\Services\Procurement\SpseCookieParser()));

        $this->assertSame(
            'application/pdf',
            $downloader->resolveMimeType(null, '%PDF-1.4 sample'),
        );
    }

    public function test_resolves_filename_from_content_disposition(): void
    {
        $downloader = new SpseDocumentDownloader(new SpseHttpClient(new \App\Services\Procurement\SpseCookieParser()));

        $filename = $downloader->resolveFilename(
            'attachment; filename="hasil-evaluasi.pdf"',
            'https://spse.inaproc.id/cianjurkab/dl/abc',
            null,
            'application/pdf',
        );

        $this->assertSame('hasil-evaluasi.pdf', $filename);
    }

    public function test_appends_extension_to_label_without_extension(): void
    {
        $downloader = new SpseDocumentDownloader(new SpseHttpClient(new \App\Services\Procurement\SpseCookieParser()));

        $filename = $downloader->resolveFilename(
            null,
            'https://spse.inaproc.id/cianjurkab/dl/abc',
            'Berita Acara Hasil Evaluasi',
            'application/pdf',
        );

        $this->assertSame('Berita Acara Hasil Evaluasi.pdf', $filename);
    }
}