<?php

namespace App\Services\OnlyOffice;

class OnlyOfficeDownloadToken
{
    public static function make(int $mediaId, int $expiresAt): string
    {
        return hash_hmac('sha256', self::payload($mediaId, $expiresAt), self::secret());
    }

    public static function valid(int $mediaId, int $expiresAt, string $token): bool
    {
        if ($expiresAt <= 0 || $expiresAt < time()) {
            return false;
        }

        if ($token === '') {
            return false;
        }

        return hash_equals(self::make($mediaId, $expiresAt), $token);
    }

    public static function buildDownloadUrl(int $mediaId): string
    {
        $expiresAt = now()
            ->addMinutes((int) config('onlyoffice.download_token_ttl_minutes', 120))
            ->getTimestamp();

        return url("/api/onlyoffice/media/{$mediaId}/download").'?'.http_build_query([
            'expires' => $expiresAt,
            'token' => self::make($mediaId, $expiresAt),
        ]);
    }

    private static function payload(int $mediaId, int $expiresAt): string
    {
        return "onlyoffice:{$mediaId}:{$expiresAt}";
    }

    private static function secret(): string
    {
        $jwtSecret = (string) config('onlyoffice.jwt_secret');

        return $jwtSecret !== '' ? $jwtSecret : (string) config('app.key');
    }
}