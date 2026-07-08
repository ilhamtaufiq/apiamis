<?php

namespace Tests\Unit;

use App\Models\Kegiatan;
use App\Services\KontrakDocumentSettingsService;
use Mockery;
use Tests\TestCase;

class KontrakDocumentSettingsServiceTest extends TestCase
{
    protected function tearDown(): void
    {
        Mockery::close();
        parent::tearDown();
    }

    public function test_resolve_pptk_uses_kegiatan_when_filled(): void
    {
        $service = Mockery::mock(KontrakDocumentSettingsService::class)->makePartial();
        $service->shouldReceive('pejabatDefaults')->andReturn([
            'nama_ppk' => 'PPK Global',
            'nip_ppk' => '111',
            'nama_pptk' => 'PPTK Settings',
            'nip_pptk' => '222',
        ]);

        $kegiatan = new Kegiatan([
            'nama_pptk' => 'PPTK Sub Kegiatan',
            'nip_pptk' => '333',
        ]);

        $resolved = $service->resolvePptk($kegiatan);

        $this->assertSame('PPTK Sub Kegiatan', $resolved['nama_pptk']);
        $this->assertSame('333', $resolved['nip_pptk']);
    }

    public function test_resolve_pptk_falls_back_to_settings_when_kegiatan_empty(): void
    {
        $service = Mockery::mock(KontrakDocumentSettingsService::class)->makePartial();
        $service->shouldReceive('pejabatDefaults')->andReturn([
            'nama_ppk' => 'PPK Global',
            'nip_ppk' => '111',
            'nama_pptk' => 'PPTK Settings',
            'nip_pptk' => '222',
        ]);

        $kegiatan = new Kegiatan([
            'nama_pptk' => null,
            'nip_pptk' => '',
        ]);

        $resolved = $service->resolvePptk($kegiatan);

        $this->assertSame('PPTK Settings', $resolved['nama_pptk']);
        $this->assertSame('222', $resolved['nip_pptk']);
    }
}