<?php

namespace Tests\Unit;

use App\Services\SpamPekerjaanIntegrationService;
use App\Services\SpmSanitasiPekerjaanIntegrationService;
use App\Support\OutputSatuan;
use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\Attributes\Test;
use PHPUnit\Framework\TestCase;

class SpmOutputClassificationTest extends TestCase
{
    /** @return array<string, array{string, ?string}> */
    public static function sanitasiKomponenProvider(): array
    {
        return [
            'ipal komunal' => ['IPAL Komunal', 'ipal'],
            'ipal dengan tanda hubung' => ['Pembangunan IPAL-Komunal', 'ipal'],
            'iplt' => ['IPLT Kabupaten', 'ipal'],
            'spald-t' => ['SPALD-T', 'ipal'],
            'spald-s' => ['SPALD-S Individu', 'tangki_septik_individu'],
            'tangki septik komunal' => ['Tangki Septik Komunal', 'tangki_septik_komunal'],
            'mck komunal' => ['MCK Komunal', 'mck_komunal'],
            // Regresi: bentuk compact "pipalateral"/"pipalingkungan" dulu cocok "ipal"
            'pipa lateral bukan ipal' => ['Pipa Lateral', null],
            'pipa lingkungan bukan ipal' => ['Pipa Lingkungan', null],
            'pipa lainnya' => ['Pipa HDPE 2"', null],
        ];
    }

    #[Test]
    #[DataProvider('sanitasiKomponenProvider')]
    public function it_classifies_sanitasi_komponen(string $komponen, ?string $expected): void
    {
        $this->assertSame($expected, SpmSanitasiPekerjaanIntegrationService::classifySanitasiKomponen($komponen));
    }

    #[Test]
    public function it_classifies_air_minum_komponen(): void
    {
        $this->assertSame('sambungan_rumah', SpamPekerjaanIntegrationService::classifyAirMinumKomponen('Sambungan Rumah'));
        $this->assertSame('bjp', SpamPekerjaanIntegrationService::classifyAirMinumKomponen('Sumur Bor'));
        $this->assertSame('reservoir', SpamPekerjaanIntegrationService::classifyAirMinumKomponen('Reservoir 20 m3'));
        $this->assertNull(SpamPekerjaanIntegrationService::classifyAirMinumKomponen('Galian Tanah'));
    }

    /** @return array<string, array{?string, bool}> */
    public static function satuanProvider(): array
    {
        return [
            'unit' => ['Unit', true],
            'sr' => ['SR', true],
            'kk' => ['KK', true],
            'buah' => ['bh', true],
            'kosong' => [null, true],
            'meter' => ['m', false],
            "meter aksen" => ["m'", false],
            'm3' => ['M3', false],
            'm superskrip' => ['m³', false],
            'meter persegi' => ['m2', false],
            'lumpsum' => ['LS', false],
            'paket' => ['Paket', false],
        ];
    }

    #[Test]
    #[DataProvider('satuanProvider')]
    public function it_detects_countable_satuan(?string $satuan, bool $expected): void
    {
        $this->assertSame($expected, OutputSatuan::isCountable($satuan));
    }
}
