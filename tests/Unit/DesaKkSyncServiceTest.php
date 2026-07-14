<?php

namespace Tests\Unit;

use App\Services\DesaKkSyncService;
use Illuminate\Support\Facades\Http;
use PHPUnit\Framework\Attributes\Test;
use Tests\TestCase;

class DesaKkSyncServiceTest extends TestCase
{
    private DesaKkSyncService $service;

    protected function setUp(): void
    {
        parent::setUp();
        $this->service = new DesaKkSyncService;
    }

    #[Test]
    public function it_normalizes_wilayah_names(): void
    {
        $this->assertSame('sukaresmi', $this->service->normalize('Kecamatan Sukaresmi'));
        $this->assertSame('rawabelut', $this->service->normalize('Desa Rawabelut'));
        $this->assertSame('cianjur', $this->service->normalize('CIANJUR'));
    }

    #[Test]
    public function it_selects_latest_tahun_and_semester(): void
    {
        $rows = [
            ['tahun' => 2023, 'semester' => 2, 'bps_nama_desa_kelurahan' => 'A'],
            ['tahun' => 2025, 'semester' => 1, 'bps_nama_desa_kelurahan' => 'B'],
            ['tahun' => 2025, 'semester' => 2, 'bps_nama_desa_kelurahan' => 'C'],
            ['tahun' => 2024, 'semester' => 2, 'bps_nama_desa_kelurahan' => 'D'],
        ];

        [$tahun, $semester, $filtered] = $this->service->selectPeriodRows($rows);

        $this->assertSame(2025, $tahun);
        $this->assertSame(2, $semester);
        $this->assertCount(1, $filtered);
        $this->assertSame('C', $filtered[0]['bps_nama_desa_kelurahan']);
    }

    #[Test]
    public function it_fetches_all_paginated_rows(): void
    {
        Http::fake([
            DesaKkSyncService::SOURCE_URL.'*' => Http::sequence()
                ->push([
                    'code' => 200,
                    'data' => [
                        ['tahun' => 2025, 'semester' => 2, 'bps_nama_desa_kelurahan' => 'A', 'jumlah_kepemilikan_kk' => 10],
                    ],
                    'pagination' => [
                        'page' => 1,
                        'per_page' => 100,
                        'total_page' => 2,
                        'total_data' => 2,
                    ],
                ])
                ->push([
                    'code' => 200,
                    'data' => [
                        ['tahun' => 2025, 'semester' => 2, 'bps_nama_desa_kelurahan' => 'B', 'jumlah_kepemilikan_kk' => 20],
                    ],
                    'pagination' => [
                        'page' => 2,
                        'per_page' => 100,
                        'total_page' => 2,
                        'total_data' => 2,
                    ],
                ]),
        ]);

        $rows = $this->service->fetchAllRows();

        $this->assertCount(2, $rows);
        $this->assertSame('A', $rows[0]['bps_nama_desa_kelurahan']);
        $this->assertSame('B', $rows[1]['bps_nama_desa_kelurahan']);
    }
}
