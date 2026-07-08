<?php

namespace Tests\Unit;

use App\Models\DocumentRegister;
use App\Models\DocumentType;
use App\Models\KontrakAddendum;
use App\Services\KontrakDocumentDataBuilder;
use App\Services\KontrakDocumentSettingsService;
use Carbon\Carbon;
use Illuminate\Support\Collection;
use Mockery;
use Tests\TestCase;

class KontrakDocumentDataBuilderTest extends TestCase
{
    protected function tearDown(): void
    {
        Mockery::close();
        parent::tearDown();
    }

    public function test_builds_ringkasan_placeholders_from_registers_and_settings(): void
    {
        $settings = Mockery::mock(KontrakDocumentSettingsService::class);
        $settings->shouldReceive('pejabatDefaults')->andReturn([
            'nama_ppk' => 'Budi PPK',
            'nip_ppk' => '198001012010011001',
            'nama_pptk' => 'Ani PPTK',
            'nip_pptk' => '198002022010011002',
        ]);
        $settings->shouldReceive('masaPemeliharaanHari')->andReturn(180);
        $settings->shouldReceive('instansiDefaults')->andReturn([
            'skpd' => 'Dinas Perumahan dan Kawasan Permukiman',
            'nomor_dpa' => '900/Kep.09/BKAD/2/2026',
            'tanggal_dpa' => '03 Februari 2026',
        ]);
        $settings->shouldReceive('caraPembayaranCheckboxData')->andReturn([
            'cara_pembayaran' => 'Sekaligus',
            'check_pembayaran_sekaligus' => '☑',
            'check_pembayaran_termin' => '☐',
            'check_pembayaran_bulan' => '☐',
        ]);

        $bastpType = new DocumentType(['code' => 'BASTP']);
        $bastpRegister = new DocumentRegister([
            'nomor' => '001/BASTP/2026',
            'tanggal' => Carbon::parse('2026-06-23'),
        ]);
        $bastpRegister->setRelation('type', $bastpType);

        $jaminanType = new DocumentType(['code' => 'JAMINAN_UM']);
        $jaminanRegister = new DocumentRegister([
            'nomor' => 'JUM/001/2026',
            'tanggal' => Carbon::parse('2026-02-10'),
        ]);
        $jaminanRegister->setRelation('type', $jaminanType);

        $bapType = new DocumentType(['code' => 'BAP']);
        $bapRegister = new DocumentRegister([
            'nomor' => '001/BAP/2026',
            'tanggal' => Carbon::parse('2026-07-06'),
        ]);
        $bapRegister->setRelation('type', $bapType);

        $addendum = new KontrakAddendum([
            'nomor_addendum' => 'ADD/001/2026',
            'tanggal_addendum' => Carbon::parse('2026-03-01'),
            'nilai_kontrak_sesudah' => 120000000,
            'tgl_selesai_sesudah' => Carbon::parse('2026-06-30'),
            'status' => 'disetujui',
        ]);

        $kegiatan = (object) [
            'nama_program' => 'Program Air Minum',
            'nama_kegiatan' => 'Kegiatan SPAM',
            'nama_sub_kegiatan' => 'Sub Kegiatan Pembangunan',
            'tahun_anggaran' => '2026',
            'sumber_dana' => 'DAK',
        ];

        $pekerjaan = (object) [
            'nama_paket' => 'Pembangunan SPAM Desa Sukamaju',
            'pagu' => 150000000,
            'kode_rekening' => '5.1.02.01',
            'kecamatan' => (object) ['nama' => 'Cianjur'],
            'desa' => (object) ['nama' => 'Sukamaju'],
            'kegiatan' => $kegiatan,
        ];

        $penyedia = (object) [
            'nama' => 'PT Contoh Jaya',
            'direktur' => 'John Doe',
            'alamat' => 'Jl. Contoh No. 1',
            'npwp' => '123456789012345',
            'bank' => 'BJB',
            'norek' => '1234567890',
            'no_akta' => '-',
            'notaris' => '-',
            'tanggal_akta' => null,
        ];

        $kontrak = Mockery::mock();
        $kontrak->penyedia = $penyedia;
        $kontrak->nilai_kontrak = 100000000;
        $kontrak->tgl_sppbj = Carbon::parse('2026-01-15');
        $kontrak->tgl_spk = Carbon::parse('2026-01-20');
        $kontrak->tgl_spmk = Carbon::parse('2026-02-01');
        $kontrak->tgl_selesai = Carbon::parse('2026-05-31');
        $kontrak->sppbj = 'SPPBJ/001/2026';
        $kontrak->spk = 'SPK/001/2026';
        $kontrak->spmk = 'SPMK/001/2026';
        $kontrak->kode_rup = '-';
        $kontrak->kode_paket = '-';
        $kontrak->nomor_penawaran = '-';
        $kontrak->tanggal_penawaran = null;
        $kontrak->shouldReceive('loadMissing')->andReturnSelf();
        $kontrak->shouldReceive('nilaiKontrakBerjalan')->andReturn(120000000.0);
        $kontrak->shouldReceive('tglSelesaiBerjalan')->andReturn(Carbon::parse('2026-06-30'));
        $kontrak->registers = new Collection([$bastpRegister, $jaminanRegister, $bapRegister]);
        $kontrak->latestApprovedAddendum = $addendum;
        $kontrak->approvedAddendums = new Collection([$addendum]);
        $kontrak->shouldReceive('relationLoaded')->with('latestApprovedAddendum')->andReturn(true);
        $kontrak->shouldReceive('relationLoaded')->with('approvedAddendums')->andReturn(true);

        $builder = new KontrakDocumentDataBuilder($settings);
        $data = $builder->build($pekerjaan, $kontrak);

        $this->assertSame('Budi PPK', $data['nama_ppk']);
        $this->assertSame('Ani PPTK', $data['nama_pptk']);
        $this->assertSame('Rp. 120.000.000', $data['nilai_kontrak_addendum']);
        $this->assertSame('Rp. 6.000.000', $data['nilai_kontrak_5persen']);
        $this->assertSame('001/BASTP/2026', $data['nomor_bastp']);
        $this->assertSame('JUM/001/2026', $data['nomor_jaminan_uang_muka']);
        $this->assertSame('001/BAP/2026', $data['nomor_bap']);
        $this->assertStringContainsString('Hari Kalender', $data['masa_hari_terbilang']);
        $this->assertStringContainsString('dari Tanggal', $data['mulai_selesai_pemeliharaan']);
        $this->assertSame('ADD/001/2026', $data['nomor_addendum1']);
        $this->assertSame('Dinas Perumahan dan Kawasan Permukiman', $data['skpd']);
        $this->assertSame('900/Kep.09/BKAD/2/2026', $data['nomor_dpa']);
        $this->assertSame('☑', $data['check_pembayaran_sekaligus']);
        $this->assertSame('☐', $data['check_pembayaran_termin']);
    }
}