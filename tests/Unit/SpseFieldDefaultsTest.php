<?php

namespace Tests\Unit;

use App\Services\Procurement\SpseFieldDefaults;
use PHPUnit\Framework\TestCase;

class SpseFieldDefaultsTest extends TestCase
{
    public function test_uses_python_default_for_satker_alamat_when_config_empty(): void
    {
        $this->assertSame(
            'Jl. Adi Sucipta No. 7 - Cianjur',
            SpseFieldDefaults::get('satker_alamat'),
        );
    }

    public function test_uses_python_default_for_sppbj_jaminan_fields(): void
    {
        $this->assertSame('0,00', SpseFieldDefaults::get('jaminan_pelaksanaan'));
        $this->assertSame('0', SpseFieldDefaults::get('masa_berlaku_jaminan'));
    }

    public function test_uses_python_default_for_spk_lingkup(): void
    {
        $this->assertSame(
            '<p>Sesuai Spesifikasi Teknis Pekerjaan</p>',
            SpseFieldDefaults::get('lingkup_pekerjaan'),
        );
    }
}