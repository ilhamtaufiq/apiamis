<?php

namespace App\Services;

use App\Models\AppSetting;
use Spatie\MediaLibrary\MediaCollections\Models\Media;

class BrandColorService
{
    public const DEFAULT_PRIMARY = '#0f766e';

    public const STORAGE_KEY = 'brand_primary_color';

    /**
     * @return array{primary: string, primary_dark: string, primary_light: string}
     */
    public static function palette(): array
    {
        $primary = self::resolvePrimaryHex();

        return self::paletteFromPrimary($primary);
    }

    public static function resolvePrimaryHex(): string
    {
        $stored = AppSetting::getValue(self::STORAGE_KEY);
        if (is_string($stored) && self::isValidHex($stored)) {
            return self::normalizeHex($stored);
        }

        $extracted = self::extractFromLogoSetting();
        if ($extracted !== null) {
            AppSetting::setValue(self::STORAGE_KEY, $extracted, 'text');

            return $extracted;
        }

        return self::DEFAULT_PRIMARY;
    }

    public static function syncFromLogoUpload(?Media $media): void
    {
        if ($media === null) {
            return;
        }

        $color = self::extractFromMedia($media);
        if ($color !== null) {
            AppSetting::setValue(self::STORAGE_KEY, $color, 'text');
        }
    }

    /**
     * @return array{primary: string, primary_dark: string, primary_light: string}
     */
    public static function paletteFromPrimary(string $primary): array
    {
        $primary = self::normalizeHex($primary);

        return [
            'primary' => $primary,
            'primary_dark' => self::darken($primary, 0.12),
            'primary_light' => self::mixWithWhite($primary, 0.82),
        ];
    }

    public static function isValidHex(string $color): bool
    {
        return (bool) preg_match('/^#?[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$/', trim($color));
    }

    public static function normalizeHex(string $color): string
    {
        $hex = ltrim(trim($color), '#');
        if (strlen($hex) === 3) {
            $hex = $hex[0].$hex[0].$hex[1].$hex[1].$hex[2].$hex[2];
        }

        return '#'.strtolower($hex);
    }

    public static function darken(string $hex, float $ratio = 0.15): string
    {
        $hex = ltrim(self::normalizeHex($hex), '#');
        $r = hexdec(substr($hex, 0, 2));
        $g = hexdec(substr($hex, 2, 2));
        $b = hexdec(substr($hex, 4, 2));

        $factor = max(0, min(1, 1 - $ratio));

        return sprintf(
            '#%02x%02x%02x',
            (int) round($r * $factor),
            (int) round($g * $factor),
            (int) round($b * $factor),
        );
    }

    public static function mixWithWhite(string $hex, float $whiteRatio = 0.75): string
    {
        $hex = ltrim(self::normalizeHex($hex), '#');
        $r = hexdec(substr($hex, 0, 2));
        $g = hexdec(substr($hex, 2, 2));
        $b = hexdec(substr($hex, 4, 2));
        $whiteRatio = max(0, min(1, $whiteRatio));
        $colorRatio = 1 - $whiteRatio;

        return sprintf(
            '#%02x%02x%02x',
            (int) round($r * $colorRatio + 255 * $whiteRatio),
            (int) round($g * $colorRatio + 255 * $whiteRatio),
            (int) round($b * $colorRatio + 255 * $whiteRatio),
        );
    }

    public static function extractFromLogoSetting(): ?string
    {
        $logoSetting = AppSetting::where('key', 'logo')->first();
        if ($logoSetting === null) {
            return null;
        }

        return self::extractFromMedia($logoSetting->getFirstMedia('app-settings'));
    }

    public static function extractFromMedia(?Media $media): ?string
    {
        if ($media === null) {
            return null;
        }

        $path = $media->getPath();
        if (! is_readable($path)) {
            return null;
        }

        $mime = (string) $media->mime_type;
        if (str_contains($mime, 'svg') || str_ends_with(strtolower($path), '.svg')) {
            return self::extractFromSvg($path);
        }

        return self::extractFromRaster($path);
    }

    private static function extractFromSvg(string $path): ?string
    {
        $content = @file_get_contents($path);
        if ($content === false || $content === '') {
            return null;
        }

        $patterns = [
            '/\bfill\s*=\s*["\']?(#[0-9a-fA-F]{3,8})/i',
            '/\bstroke\s*=\s*["\']?(#[0-9a-fA-F]{3,8})/i',
            '/#([0-9a-fA-F]{6})\b/',
        ];

        foreach ($patterns as $pattern) {
            if (preg_match($pattern, $content, $matches)) {
                $candidate = str_starts_with($matches[1], '#') ? $matches[1] : '#'.$matches[1];
                if (self::isValidHex($candidate) && ! self::isNeutralHex(self::normalizeHex($candidate))) {
                    return self::normalizeHex($candidate);
                }
            }
        }

        return null;
    }

    private static function extractFromRaster(string $path): ?string
    {
        if (! extension_loaded('gd')) {
            return null;
        }

        $data = @file_get_contents($path);
        if ($data === false) {
            return null;
        }

        $image = @imagecreatefromstring($data);
        if ($image === false) {
            return null;
        }

        $width = imagesx($image);
        $height = imagesy($image);
        $targetW = max(1, min(64, $width));
        $targetH = max(1, min(64, $height));

        $resized = imagecreatetruecolor($targetW, $targetH);
        imagealphablending($resized, false);
        imagesavealpha($resized, true);
        imagecopyresampled($resized, $image, 0, 0, 0, 0, $targetW, $targetH, $width, $height);

        $buckets = [];
        for ($y = 0; $y < $targetH; $y++) {
            for ($x = 0; $x < $targetW; $x++) {
                $rgba = imagecolorat($resized, $x, $y);
                $alpha = ($rgba >> 24) & 0x7F;
                if ($alpha > 96) {
                    continue;
                }

                $r = ($rgba >> 16) & 0xFF;
                $g = ($rgba >> 8) & 0xFF;
                $b = $rgba & 0xFF;

                if (self::isNeutralRgb($r, $g, $b)) {
                    continue;
                }

                $bucket = sprintf('%02x%02x%02x', $r, $g, $b);
                $buckets[$bucket] = ($buckets[$bucket] ?? 0) + 1;
            }
        }

        imagedestroy($image);
        imagedestroy($resized);

        if ($buckets === []) {
            return null;
        }

        arsort($buckets);

        return '#'.array_key_first($buckets);
    }

    private static function isNeutralHex(string $hex): bool
    {
        $hex = ltrim(self::normalizeHex($hex), '#');
        $r = hexdec(substr($hex, 0, 2));
        $g = hexdec(substr($hex, 2, 2));
        $b = hexdec(substr($hex, 4, 2));

        return self::isNeutralRgb($r, $g, $b);
    }

    private static function isNeutralRgb(int $r, int $g, int $b): bool
    {
        $max = max($r, $g, $b);
        $min = min($r, $g, $b);

        if (($max - $min) < 28) {
            return true;
        }

        if ($max > 238 && $min > 205) {
            return true;
        }

        return $max < 36;
    }
}