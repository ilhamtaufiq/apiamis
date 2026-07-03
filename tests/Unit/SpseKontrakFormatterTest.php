<?php

namespace Tests\Unit;

use App\Services\Procurement\SpseKontrakFormatter;
use Carbon\Carbon;
use PHPUnit\Framework\TestCase;

class SpseKontrakFormatterTest extends TestCase
{
    public function test_formats_date_and_nilai_for_spse(): void
    {
        $formatter = new SpseKontrakFormatter();

        $this->assertSame('02-07-2026', $formatter->formatDate(Carbon::parse('2026-07-02')));
        $this->assertSame('149444256,15', $formatter->formatNilai(149444256.15));
        $this->assertSame(149444256.15, $formatter->parseNilai('149444256,15'));
    }
}