<?php

namespace Tests\Unit;

use App\Services\Procurement\SpseKontrakHtmlParser;
use PHPUnit\Framework\TestCase;

class SpseKontrakHtmlParserTest extends TestCase
{
    private SpseKontrakHtmlParser $parser;

    protected function setUp(): void
    {
        parent::setUp();
        $this->parser = new SpseKontrakHtmlParser();
    }

    public function test_extracts_sppbj_and_spk_ids_from_list_html(): void
    {
        $html = '<a href="/spk-pl/spkpl?sppbjId=10505359000">SPK</a> edit?spkId=10449553000';

        $ids = $this->parser->extractIdsFromListHtml($html);

        $this->assertSame('10505359000', $ids['sppbj_id']);
        $this->assertSame('10449553000', $ids['spk_id']);
    }

    public function test_resolves_rekanan_id_by_penyedia_name(): void
    {
        $html = '<select name="rekananId"><option value="0">Pilih</option>'
            .'<option value="401418">PT AIR MINUM SEJAHTERA</option></select>';

        $id = $this->parser->resolveRekananId($html, 'PT Air Minum Sejahtera');

        $this->assertSame('401418', $id);
    }

    public function test_extracts_spk_nilai_from_form_html(): void
    {
        $html = '<input type="text" readonly name="spk.spk_nilai" value="149444256,15" />';

        $this->assertSame('149444256,15', $this->parser->extractSpkNilai($html));
    }

    public function test_prefers_nilai_kontrak_field_from_spse_form(): void
    {
        $html = '<input id="nilaiKontrak_f" value="149444256,15" />'
            .'<input name="spk.spk_nilai" value="0,00" />';

        $this->assertSame('149444256,15', $this->parser->extractNilaiKontrak($html));
    }
}