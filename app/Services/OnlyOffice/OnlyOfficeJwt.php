<?php

namespace App\Services\OnlyOffice;

class OnlyOfficeJwt
{
    public static function encode(array $payload, string $secret): string
    {
        $header = self::base64UrlEncode(json_encode(['alg' => 'HS256', 'typ' => 'JWT'], JSON_UNESCAPED_SLASHES));
        $body = self::base64UrlEncode(json_encode($payload, JSON_UNESCAPED_SLASHES));
        $signature = self::base64UrlEncode(hash_hmac('sha256', "{$header}.{$body}", $secret, true));

        return "{$header}.{$body}.{$signature}";
    }

    public static function decode(string $token, string $secret): ?array
    {
        $parts = explode('.', $token);
        if (count($parts) !== 3) {
            return null;
        }

        [$header, $body, $signature] = $parts;
        $expected = self::base64UrlEncode(hash_hmac('sha256', "{$header}.{$body}", $secret, true));

        if (! hash_equals($expected, $signature)) {
            return null;
        }

        $decoded = json_decode(self::base64UrlDecode($body), true);

        if (! is_array($decoded)) {
            return null;
        }

        // Reject expired tokens when an exp claim is present.
        if (isset($decoded['exp']) && is_numeric($decoded['exp']) && (int) $decoded['exp'] < time()) {
            return null;
        }

        return $decoded;
    }

    private static function base64UrlEncode(string $value): string
    {
        return rtrim(strtr(base64_encode($value), '+/', '-_'), '=');
    }

    private static function base64UrlDecode(string $value): string
    {
        $remainder = strlen($value) % 4;
        if ($remainder > 0) {
            $value .= str_repeat('=', 4 - $remainder);
        }

        return base64_decode(strtr($value, '-_', '+/')) ?: '';
    }
}