<?php

namespace Tests\Unit;

use App\Services\Procurement\SpseDocumentZipService;
use PHPUnit\Framework\TestCase;

class SpseDocumentZipServiceTest extends TestCase
{
    public function test_unique_entry_name_adds_index_prefix(): void
    {
        $service = new SpseDocumentZipService(new \App\Services\Procurement\SpseDocumentDownloader(
            new \App\Services\Procurement\SpseHttpClient(new \App\Services\Procurement\SpseCookieParser()),
        ));
        $used = [];

        $name = $service->uniqueEntryName($used, 3, 'Berita Acara.pdf');

        $this->assertSame('03_Berita Acara.pdf', $name);
    }

    public function test_unique_entry_name_avoids_collision(): void
    {
        $service = new SpseDocumentZipService(new \App\Services\Procurement\SpseDocumentDownloader(
            new \App\Services\Procurement\SpseHttpClient(new \App\Services\Procurement\SpseCookieParser()),
        ));
        $used = ['03_laporan.pdf' => true];

        $name = $service->uniqueEntryName($used, 3, 'laporan.pdf');

        $this->assertSame('03_laporan_2.pdf', $name);
    }
}