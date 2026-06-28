<?php

namespace Tests\Unit;

use App\Services\OnlyOffice\OnlyOfficeDownloadToken;
use PHPUnit\Framework\Attributes\Test;
use Tests\TestCase;

class OnlyOfficeDownloadTokenTest extends TestCase
{
    #[Test]
    public function it_accepts_a_valid_token_for_media_download(): void
    {
        config([
            'onlyoffice.jwt_secret' => 'test-onlyoffice-secret',
            'onlyoffice.download_token_ttl_minutes' => 120,
            'app.url' => 'https://apiamis.example.test',
        ]);

        $expiresAt = now()->addHour()->getTimestamp();
        $token = OnlyOfficeDownloadToken::make(7703, $expiresAt);

        $this->assertTrue(OnlyOfficeDownloadToken::valid(7703, $expiresAt, $token));
    }

    #[Test]
    public function it_rejects_expired_or_tampered_tokens(): void
    {
        config(['onlyoffice.jwt_secret' => 'test-onlyoffice-secret']);

        $expiresAt = now()->subMinute()->getTimestamp();
        $token = OnlyOfficeDownloadToken::make(7703, $expiresAt);

        $this->assertFalse(OnlyOfficeDownloadToken::valid(7703, $expiresAt, $token));
        $this->assertFalse(OnlyOfficeDownloadToken::valid(7703, now()->addHour()->getTimestamp(), 'invalid-token'));
    }

    #[Test]
    public function it_builds_absolute_download_urls_with_token_query(): void
    {
        config([
            'onlyoffice.jwt_secret' => 'test-onlyoffice-secret',
            'onlyoffice.download_token_ttl_minutes' => 120,
            'app.url' => 'https://apiamis.example.test',
        ]);

        $downloadUrl = OnlyOfficeDownloadToken::buildDownloadUrl(42);
        $path = (string) parse_url($downloadUrl, PHP_URL_PATH);

        $this->assertSame('/api/onlyoffice/media/42/download', $path);
        parse_str((string) parse_url($downloadUrl, PHP_URL_QUERY), $query);

        $this->assertArrayHasKey('expires', $query);
        $this->assertArrayHasKey('token', $query);
        $this->assertTrue(OnlyOfficeDownloadToken::valid(42, (int) $query['expires'], (string) $query['token']));
    }
}