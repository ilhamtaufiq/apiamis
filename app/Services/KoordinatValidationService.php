<?php

namespace App\Services;

use App\Models\Pekerjaan;

class KoordinatValidationService
{
    private static ?array $villageIndex = null;

    /**
     * @return array{valid: bool, message: string}
     */
    public function validateForPekerjaan(Pekerjaan $pekerjaan, string $koordinat): array
    {
        $coords = $this->parseKoordinat($koordinat);
        if ($coords === null) {
            return [
                'valid' => false,
                'message' => 'Koordinat tidak dapat dibaca. Gunakan format lat, lng.',
            ];
        }

        $pekerjaan->loadMissing(['desa', 'kecamatan']);
        $desaName = $pekerjaan->desa?->n_desa;
        $kecName = $pekerjaan->kecamatan?->n_kec;

        if (! $desaName || ! $kecName) {
            return [
                'valid' => false,
                'message' => 'Data desa/kecamatan pekerjaan belum lengkap.',
            ];
        }

        $feature = $this->findVillageFeature($kecName, $desaName);
        if ($feature === null) {
            return [
                'valid' => false,
                'message' => "Batas desa {$desaName} tidak ditemukan di peta.",
            ];
        }

        if ($this->pointInsideFeature($coords['lng'], $coords['lat'], $feature)) {
            return [
                'valid' => true,
                'message' => "Koordinat berada di Desa {$desaName}, Kec. {$kecName}.",
            ];
        }

        return [
            'valid' => false,
            'message' => "Koordinat di luar Desa {$desaName}, Kec. {$kecName}.",
        ];
    }

    /**
     * @return array{lat: float, lng: float}|null
     */
    public function parseKoordinat(string $value): ?array
    {
        $trimmed = trim($value);
        if ($trimmed === '' || strcasecmp($trimmed, 'manual') === 0) {
            return null;
        }

        if (preg_match('/(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)/', $trimmed, $matches) === 1) {
            $lat = (float) $matches[1];
            $lng = (float) $matches[2];

            return $this->normalizeLatLng($lat, $lng);
        }

        $cleaned = preg_replace('/\s+/', '', $trimmed) ?? '';
        if (preg_match('/10\d\.\d+/', $cleaned, $lngMatch, PREG_OFFSET_CAPTURE) === 1) {
            $lngMarker = $lngMatch[0][1];
            $lat = (float) substr($cleaned, 0, $lngMarker);
            $lng = (float) substr($cleaned, $lngMarker);

            return $this->normalizeLatLng($lat, $lng);
        }

        return null;
    }

    /**
     * @return array{lat: float, lng: float}|null
     */
    private function normalizeLatLng(float $lat, float $lng): ?array
    {
        if (! is_finite($lat) || ! is_finite($lng)) {
            return null;
        }

        [$lat, $lng] = $this->correctIndonesiaCoordSigns($lat, $lng);

        if ($lat < -90 || $lat > 90 || $lng < -180 || $lng > 180) {
            return null;
        }

        return ['lat' => $lat, 'lng' => $lng];
    }

    /**
     * OCR often drops the minus on Java/Bali latitude (southern hemisphere).
     *
     * @return array{0: float, 1: float}
     */
    private function correctIndonesiaCoordSigns(float $lat, float $lng): array
    {
        if ($lat > 0 && $lat <= 12 && $lng >= 104 && $lng <= 115) {
            $lat = -$lat;
        }

        if ($lng < 0 && $lng >= -115 && $lng <= -104) {
            $lng = -$lng;
        }

        return [$lat, $lng];
    }

    private function normalizeWilayahName(?string $value): string
    {
        if (! $value) {
            return '';
        }

        $value = str_ireplace(['kecamatan', 'desa', 'kelurahan'], '', $value);

        return strtolower(preg_replace('/[^a-z0-9]+/i', '', $value) ?? '');
    }

    /**
     * @return array<string, array<string, mixed>>
     */
    private function loadVillageIndex(): array
    {
        if (self::$villageIndex !== null) {
            return self::$villageIndex;
        }

        $path = resource_path('geojson/id3203_cianjur_simplified.geojson');
        if (! is_readable($path)) {
            self::$villageIndex = [];

            return self::$villageIndex;
        }

        $raw = file_get_contents($path);
        $data = is_string($raw) ? json_decode($raw, true) : null;
        $index = [];

        foreach (($data['features'] ?? []) as $feature) {
            $district = (string) ($feature['properties']['district'] ?? '');
            $village = (string) ($feature['properties']['village'] ?? '');
            $key = $this->normalizeWilayahName($district).'|'.$this->normalizeWilayahName($village);
            $index[$key] = $feature;
        }

        self::$villageIndex = $index;

        return self::$villageIndex;
    }

    /**
     * @return array<string, mixed>|null
     */
    private function findVillageFeature(string $kecName, string $desaName): ?array
    {
        $key = $this->normalizeWilayahName($kecName).'|'.$this->normalizeWilayahName($desaName);

        return $this->loadVillageIndex()[$key] ?? null;
    }

    /**
     * @param  array<string, mixed>  $feature
     */
    private function pointInsideFeature(float $lng, float $lat, array $feature): bool
    {
        $geometry = $feature['geometry'] ?? null;
        if (! is_array($geometry)) {
            return false;
        }

        $type = $geometry['type'] ?? null;
        $coordinates = $geometry['coordinates'] ?? null;
        if (! is_array($coordinates)) {
            return false;
        }

        return match ($type) {
            'Polygon' => $this->pointInPolygonRings($lng, $lat, $coordinates),
            'MultiPolygon' => $this->pointInMultiPolygon($lng, $lat, $coordinates),
            default => false,
        };
    }

    /**
     * @param  array<int, array<int, array{0: float|int, 1: float|int}>>  $multiPolygon
     */
    private function pointInMultiPolygon(float $lng, float $lat, array $multiPolygon): bool
    {
        foreach ($multiPolygon as $polygon) {
            if (! is_array($polygon)) {
                continue;
            }

            if ($this->pointInPolygonRings($lng, $lat, $polygon)) {
                return true;
            }
        }

        return false;
    }

    /**
     * @param  array<int, array<int, array{0: float|int, 1: float|int}>>  $rings
     */
    private function pointInPolygonRings(float $lng, float $lat, array $rings): bool
    {
        if ($rings === [] || ! isset($rings[0]) || ! is_array($rings[0])) {
            return false;
        }

        if (! $this->pointInRing($lng, $lat, $rings[0])) {
            return false;
        }

        $ringCount = count($rings);
        for ($i = 1; $i < $ringCount; $i++) {
            if (isset($rings[$i]) && is_array($rings[$i]) && $this->pointInRing($lng, $lat, $rings[$i])) {
                return false;
            }
        }

        return true;
    }

    /**
     * @param  array<int, array{0: float|int, 1: float|int}>  $ring
     */
    private function pointInRing(float $lng, float $lat, array $ring): bool
    {
        $inside = false;
        $count = count($ring);
        if ($count < 3) {
            return false;
        }

        for ($i = 0, $j = $count - 1; $i < $count; $j = $i++) {
            $xi = (float) $ring[$i][0];
            $yi = (float) $ring[$i][1];
            $xj = (float) $ring[$j][0];
            $yj = (float) $ring[$j][1];

            $denominator = $yj - $yi;
            if (abs($denominator) < 1e-12) {
                continue;
            }

            $intersect = (($yi > $lat) !== ($yj > $lat))
                && ($lng < ($xj - $xi) * ($lat - $yi) / $denominator + $xi);

            if ($intersect) {
                $inside = ! $inside;
            }
        }

        return $inside;
    }
}