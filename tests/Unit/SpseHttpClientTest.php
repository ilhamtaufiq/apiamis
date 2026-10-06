<?php

namespace Tests\Unit;

use App\Services\Procurement\SpseHttpClient;
use PHPUnit\Framework\TestCase;

class SpseHttpClientTest extends TestCase
{
    public function test_extracts_token_from_datatable_js_snippet(): void
    {
        $client = new SpseHttpClient(new \App\Services\Procurement\SpseCookieParser());
        $html = "data: function (d) { d.authenticityToken = '58dcb9540836f09b4721e0d9ba7cd6e616be19b6'; },";

        $this->assertSame(
            '58dcb9540836f09b4721e0d9ba7cd6e616be19b6',
            $client->extractTokenFromHtml($html),
        );
    }

    public function test_detects_login_page_but_not_datatable_json(): void
    {
        $client = new SpseHttpClient(new \App\Services\Procurement\SpseCookieParser());

        $this->assertTrue($client->looksLikeLoginPage('<html><body><form id="loginCtr"></form></body></html>'));
        $this->assertFalse($client->looksLikeLoginPage('{"draw":"1","data":[["1","Paket loginctr"]]}'));
        $this->assertFalse($client->looksLikeLoginPage(''));
    }
}
