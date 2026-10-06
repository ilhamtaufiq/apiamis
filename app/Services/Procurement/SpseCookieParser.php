<?php

namespace App\Services\Procurement;

class SpseCookieParser
{
    /**
     * @param  array<int, array{name: string, value: string, domain?: string, path?: string}>|null  $structured
     * @return array<int, array{name: string, value: string, domain: string, path: string}>
     */
    public function parse(?string $cookieHeader, ?array $structured = null): array
    {
        if (is_array($structured) && count($structured) > 0) {
            return array_values(array_map(function (array $cookie) {
                return [
                    'name' => (string) ($cookie['name'] ?? ''),
                    'value' => (string) ($cookie['value'] ?? ''),
                    'domain' => (string) ($cookie['domain'] ?? '.inaproc.id'),
                    'path' => (string) ($cookie['path'] ?? '/'),
                ];
            }, array_filter($structured, fn ($c) => ! empty($c['name'] ?? null))));
        }

        if (! $cookieHeader) {
            return [];
        }

        $cookies = [];
        // Hasil copy DevTools bisa berisi prefix "Cookie:", baris baru, atau tanda kutip pembungkus.
        $cookieHeader = trim(preg_replace('/^\s*cookie\s*:\s*/i', '', trim($cookieHeader)) ?? '');
        $cookieHeader = trim(str_replace(["\r", "\n"], '', $cookieHeader), "\"' ");

        foreach (explode(';', $cookieHeader) as $part) {
            $part = trim($part);
            if ($part === '' || ! str_contains($part, '=')) {
                continue;
            }

            [$name, $value] = explode('=', $part, 2);
            $name = trim($name);
            if ($name === '' || preg_match('/[\s,"]/', $name)) {
                continue;
            }

            $domain = str_starts_with(strtolower($name), 'play_') || $name === 'sirup_instansi_id'
                ? 'sirup.inaproc.id'
                : (strtoupper($name) === 'SPSE_SESSION' ? 'spse.inaproc.id' : '.inaproc.id');

            $cookies[] = [
                'name' => $name,
                'value' => trim($value),
                'domain' => $domain,
                'path' => '/',
            ];
        }

        return $cookies;
    }

    public function toHeader(array $cookies): string
    {
        return collect($cookies)
            ->map(fn (array $c) => $c['name'].'='.$c['value'])
            ->implode('; ');
    }
}