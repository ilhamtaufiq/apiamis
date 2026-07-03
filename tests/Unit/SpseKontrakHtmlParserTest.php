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

    public function test_detects_complete_kontrak_status_from_list_html(): void
    {
        $html = '<table id="tblsppbj"><tbody><tr>'
            .'<td><a href="/sppbj-pl/sppbjppkpl?plId=10919928000&sppbjId=10505359000">SPPBJ</a></td>'
            .'<td><a href="/spk-pl/editspk?spkId=10449553000&sppbjId=10505359000">SPK</a></td>'
            .'<td>SSKK/SUSPK</td><td><a>Sekaligus</a></td>'
            .'<td><a href="/spk-pl/editspmknonpl?spkId=10449553000&sppbjId=10505359000&pesananId=10436117000">SPMK</a></td>'
            .'</tr></tbody></table>';

        $status = $this->parser->extractKontrakListStatus($html);

        $this->assertTrue($status['sppbj_complete']);
        $this->assertTrue($status['spk_complete']);
        $this->assertTrue($status['sskk_complete']);
        $this->assertTrue($status['spmk_complete']);
        $this->assertTrue($status['all_complete']);
        $this->assertSame('10436117000', $status['pesanan_id']);
    }

    public function test_detects_partial_kontrak_status_from_list_html(): void
    {
        $html = '<table id="tblsppbj"><tbody><tr>'
            .'<td><a href="/spk-pl/spkpl?sppbjId=10505359000">SPK</a></td>'
            .'</tr></tbody></table>';

        $status = $this->parser->extractKontrakListStatus($html);

        $this->assertTrue($status['sppbj_complete']);
        $this->assertFalse($status['spk_complete']);
        $this->assertFalse($status['all_complete']);
    }

    public function test_resolves_rekanan_id_by_penyedia_name(): void
    {
        $html = '<select name="rekananId"><option value="0">Pilih</option>'
            .'<option value="401418">PT AIR MINUM SEJAHTERA</option></select>';

        $id = $this->parser->resolveRekananId($html, 'PT Air Minum Sejahtera');

        $this->assertSame('401418', $id);
    }

    public function test_prefers_hidden_rekanan_id_without_penyedia_name_match(): void
    {
        $html = '<input type="hidden" name="rekananId" value="401418" />'
            .'<select name="rekananId" disabled><option value="0">Pilih</option></select>';

        $id = $this->parser->resolveRekananId($html, 'CV. PUTRA SILIWANGI PADJAJARAN');

        $this->assertSame('401418', $id);
    }

    public function test_uses_single_rekanan_option_when_name_differs(): void
    {
        $html = '<select name="rekananId"><option value="0">Pilih</option>'
            .'<option value="882211" selected>CV PUTRA SILIWANGI</option></select>';

        $id = $this->parser->resolveRekananId($html, 'CV. PUTRA SILIWANGI PADJAJARAN');

        $this->assertSame('882211', $id);
    }

    public function test_prefers_stored_rekanan_id_over_form_values(): void
    {
        $html = '<input type="hidden" name="rekananId" value="401418" />';

        $id = $this->parser->resolveRekananId($html, 'Nama Berbeda', '999999');

        $this->assertSame('999999', $id);
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

    public function test_extracts_sppbj_id_from_redirect_location(): void
    {
        $location = 'https://spse.inaproc.id/cianjurkab/sppbj-pl/sppbjppkpl?plId=10919928000&sppbjId=10505359000';

        $this->assertSame('10505359000', $this->parser->extractQueryParam($location, 'sppbjId'));
    }
}