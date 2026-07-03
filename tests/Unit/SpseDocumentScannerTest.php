<?php

namespace Tests\Unit;

use App\Services\Procurement\SpseDocumentScanner;
use App\Services\Procurement\SpseHttpClient;
use PHPUnit\Framework\TestCase;

class SpseDocumentScannerTest extends TestCase
{
    private SpseDocumentScanner $scanner;

    protected function setUp(): void
    {
        parent::setUp();
        $this->scanner = new SpseDocumentScanner(new SpseHttpClient(new \App\Services\Procurement\SpseCookieParser()));
    }

    public function test_discovers_nontender_viewpdfpl_and_dl_links(): void
    {
        $html = <<<'HTML'
            <a href="/cianjurkab/viewpdfpl/abc">Summary Non Tender</a>
            <a href="/cianjurkab/dl/hash123">Berita Acara Hasil Evaluasi Penawaran.pdf</a>
        HTML;

        $docs = $this->scanner->discoverFromNontenderHtml($html);

        $this->assertCount(2, $docs);
        $this->assertSame('summary', $docs[0]['doc_type']);
        $this->assertSame('dl', $docs[1]['doc_type']);
    }

    public function test_discovers_penawaran_rincian_and_cetak_links(): void
    {
        $html = <<<'HTML'
            <a href="/cianjurkab/pesertanontender/1/cetaksuratpenawaranpeserta">Cetak</a>
            <a href="/cianjurkab/rincian_adminteknis/99">Admin Teknis</a>
            <a href="/cianjurkab/rincian_penawaran/99">Harga</a>
        HTML;

        $docs = $this->scanner->discoverFromPenawaranHtml($html);

        $this->assertCount(3, $docs);
        $this->assertSame('surat_penawaran', $docs[0]['doc_type']);
        $this->assertSame('admin_teknis', $docs[1]['doc_type']);
        $this->assertSame('harga', $docs[2]['doc_type']);
    }

    public function test_discovers_evaluasi_dlsec_with_label_only(): void
    {
        $html = <<<'HTML'
            <a href="/cianjurkab/dlsec/aaa">BUKTI SEWA DAN KEPEMILIKAN ALAT.pdf</a>
            <a href="/cianjurkab/dlsec/bbb"></a>
        HTML;

        $docs = $this->scanner->discoverFromEvaluasiHtml($html);

        $this->assertCount(1, $docs);
        $this->assertSame('BUKTI SEWA DAN KEPEMILIKAN ALAT.pdf', $docs[0]['label']);
    }

    public function test_discovers_kualifikasi_cetak_and_dl(): void
    {
        $html = <<<'HTML'
            <a href="/cianjurkab/cetakkualifikasipl/55">Kualifikasi</a>
            <a href="/cianjurkab/dl/xyz">Persyaratan tambahan.pdf</a>
        HTML;

        $docs = $this->scanner->discoverFromKualifikasiHtml($html);

        $this->assertCount(2, $docs);
        $this->assertSame('kualifikasi', $docs[0]['doc_type']);
        $this->assertSame('dl_kualifikasi', $docs[1]['doc_type']);
    }

    public function test_pages_for_pengadaan_langsung(): void
    {
        $pages = $this->scanner->pagesForJenis('pengadaan_langsung', '10908686000');

        $this->assertSame('/nontender/10908686000', $pages[0]['path']);
        $this->assertSame('/pesertanontender/10908686000/penawaran', $pages[1]['path']);
    }

    public function test_pages_for_tender_seleksi(): void
    {
        $pages = $this->scanner->pagesForTender('12345');

        $this->assertSame('/tender/12345', $pages[0]['path']);
    }
}