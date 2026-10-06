<?php

namespace Tests\Unit;

use App\Services\Procurement\SpseCookieParser;
use PHPUnit\Framework\TestCase;

class SpseCookieParserTest extends TestCase
{
    public function test_parses_cookie_header(): void
    {
        $parser = new SpseCookieParser();
        $cookies = $parser->parse('SPSE_SESSION=abc123; XSRF-TOKEN=xyz');

        $this->assertCount(2, $cookies);
        $this->assertSame('SPSE_SESSION', $cookies[0]['name']);
        $this->assertSame('abc123', $cookies[0]['value']);
    }

    public function test_to_header_roundtrip(): void
    {
        $parser = new SpseCookieParser();
        $cookies = [
            ['name' => 'SPSE_SESSION', 'value' => 'a', 'domain' => 'spse.inaproc.id', 'path' => '/'],
        ];

        $this->assertSame('SPSE_SESSION=a', $parser->toHeader($cookies));
    }

    public function test_strips_cookie_prefix_and_newlines_from_pasted_header(): void
    {
        $parser = new SpseCookieParser();
        $cookies = $parser->parse("Cookie: SPSE_SESSION=abc-___AT=1&x=2;\n foo=bar");

        $this->assertSame('SPSE_SESSION', $cookies[0]['name']);
        $this->assertSame('abc-___AT=1&x=2', $cookies[0]['value']);
        $this->assertSame('foo', $cookies[1]['name']);
    }
}
