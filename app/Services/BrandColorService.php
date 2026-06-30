<?php

namespace App\Services;

use App\Models\AppSetting;
use Spatie\MediaLibrary\MediaCollections\Models\Media;

class BrandColorService
{
    /** Ungu brand Arumanis (selaras logo), bukan pink/magenta. */
    public const DEFAULT_PRIMARY = '#674bb5';

    public const STORAGE_KEY = 'brand_primary_color';

    /**
     * @return array{
     *     primary: string,
     *     primary_dark: string,
     *     primary_light: string,
     *     primary_fixed: string,
     *     primary_container: string,
     *     secondary: string,
     *     secondary_fixed: string,
     *     on_secondary: string,
     *     on_secondary_fixed: string,
     *     surface: string,
     *     surface_container: string,
     *     surface_container_low: string,
     *     outline_variant: string,
     *     on_surface: string,
     *     on_surface_variant: string
     * }
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
            return self::persistBrandPrimary(self::normalizeHex($stored));
        }

        $extracted = self::extractFromLogoSetting();
        if ($extracted !== null) {
            return self::persistBrandPrimary($extracted);
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
            self::persistBrandPrimary($color);
        }
    }

    public static function sanitizeBrandPrimary(string $hex): string
    {
        $hex = self::normalizeHex($hex);

        if (self::isPinkish($hex) || self::relativeLuminance($hex) > 0.42) {
            return self::DEFAULT_PRIMARY;
        }

        return $hex;
    }

    private static function persistBrandPrimary(string $hex): string
    {
        $sanitized = self::sanitizeBrandPrimary($hex);
        AppSetting::setValue(self::STORAGE_KEY, $sanitized, 'text');

        return $sanitized;
    }

    /**
     * @return array{
     *     primary: string,
     *     primary_dark: string,
     *     primary_light: string,
     *     primary_fixed: string,
     *     primary_container: string,
     *     secondary: string,
     *     secondary_fixed: string,
     *     on_secondary: string,
     *     on_secondary_fixed: string,
     *     surface: string,
     *     surface_container: string,
     *     surface_container_low: string,
     *     outline_variant: string,
     *     on_surface: string,
     *     on_surface_variant: string
     * }
     */
    public static function paletteFromPrimary(string $primary): array
    {
        $primary = self::normalizeHex($primary);
        $secondary = self::darken($primary, 0.22);

        return [
            'primary' => $primary,
            'primary_dark' => self::darken($primary, 0.14),
            'primary_light' => self::mixWithWhite($primary, 0.78),
            'primary_fixed' => self::mixWithWhite($primary, 0.88),
            'primary_container' => self::mixWithWhite($primary, 0.86),
            'secondary' => $secondary,
            'secondary_fixed' => self::mixWithWhite($secondary, 0.78),
            'on_secondary' => '#ffffff',
            'on_secondary_fixed' => self::darken($secondary, 0.08),
            'surface' => self::mixWithWhite($primary, 0.965),
            'surface_container' => self::mixWithWhite($primary, 0.9),
            'surface_container_low' => self::mixWithWhite($primary, 0.93),
            'outline_variant' => self::mixWithWhite($primary, 0.72),
            'on_surface' => '#1f1926',
            'on_surface_variant' => '#50434b',
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

        $bestBucket = null;
        $bestScore = -1.0;
        foreach ($buckets as $bucket => $count) {
            $r = hexdec(substr($bucket, 0, 2));
            $g = hexdec(substr($bucket, 2, 2));
            $b = hexdec(substr($bucket, 4, 2));
            $score = $count * self::purpleHueScore($r, $g, $b);
            if ($score > $bestScore) {
                $bestScore = $score;
                $bestBucket = $bucket;
            }
        }

        return $bestBucket !== null ? '#'.$bestBucket : null;
    }

    private static function purpleHueScore(int $r, int $g, int $b): float
    {
        $hue = self::rgbHue($r, $g, $b);
        if ($hue >= 230 && $hue <= 295) {
            return 1.35;
        }

        if ($hue >= 295 && $hue <= 340) {
            return 0.45;
        }

        return 0.85;
    }

    private static function rgbHue(int $r, int $g, int $b): float
    {
        $r /= 255;
        $g /= 255;
        $b /= 255;
        $max = max($r, $g, $b);
        $min = min($r, $g, $b);
        $delta = $max - $min;

        if ($delta < 0.00001) {
            return 0.0;
        }

        if ($max === $r) {
            $hue = 60 * fmod((($g - $b) / $delta), 6);
        } elseif ($max === $g) {
            $hue = 60 * ((($b - $r) / $delta) + 2);
        } else {
            $hue = 60 * ((($r - $g) / $delta) + 4);
        }

        if ($hue < 0) {
            $hue += 360;
        }

        return $hue;
    }

    private static function isPinkish(string $hex): bool
    {
        $hex = ltrim(self::normalizeHex($hex), '#');
        $r = hexdec(substr($hex, 0, 2));
        $g = hexdec(substr($hex, 2, 2));
        $b = hexdec(substr($hex, 4, 2));
        $hue = self::rgbHue($r, $g, $b);

        return $hue >= 300 && $hue <= 360 && $r > $b;
    }

    private static function relativeLuminance(string $hex): float
    {
        $hex = ltrim(self::normalizeHex($hex), '#');
        $channels = [];
        foreach ([0, 2, 4] as $offset) {
            $value = hexdec(substr($hex, $offset, 2)) / 255;
            $channels[] = $value <= 0.03928
                ? $value / 12.92
                : (($value + 0.055) / 1.055) ** 2.4;
        }

        return 0.2126 * $channels[0] + 0.7152 * $channels[1] + 0.0722 * $channels[2];
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