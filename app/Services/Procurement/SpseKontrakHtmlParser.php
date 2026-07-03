<?php

namespace App\Services\Procurement;

class SpseKontrakHtmlParser
{
    /**
     * @return array{sppbj_id: ?string, spk_id: ?string}
     */
    public function extractIdsFromListHtml(string $html): array
    {
        return [
            'sppbj_id' => $this->firstMatch($html, '/sppbjId=(\d+)/i'),
            'spk_id' => $this->firstMatch($html, '/spkId=(\d+)/i'),
        ];
    }

    /**
     * @return array<int, array{id: string, label: string}>
     */
    public function extractRekananOptions(string $html): array
    {
        $options = [];

        if (preg_match_all(
            '/<option[^>]*value=["\'](\d+)["\'][^>]*>(.*?)<\/option>/is',
            $html,
            $matches,
            PREG_SET_ORDER,
        )) {
            foreach ($matches as $match) {
                $id = trim($match[1]);
                if ($id === '' || $id === '0') {
                    continue;
                }
                $label = trim(html_entity_decode(strip_tags($match[2])));
                $options[] = ['id' => $id, 'label' => $label];
            }
        }

        if (preg_match('/name=["\']rekananId["\'][^>]*value=["\'](\d+)["\']/i', $html, $hidden)) {
            $options[] = ['id' => $hidden[1], 'label' => ''];
        }

        return $options;
    }

    public function extractHiddenValue(string $html, string $name): ?string
    {
        return $this->extractInputValue($html, $name);
    }

    public function extractInputValue(string $html, string $name): ?string
    {
        $escaped = preg_quote($name, '/');
        if (preg_match('/name=["\']'.$escaped.'["\'][^>]*value=["\']([^"\']*)["\']/i', $html, $m)) {
            return $m[1] !== '' ? $m[1] : null;
        }

        if (preg_match('/value=["\']([^"\']*)["\'][^>]*name=["\']'.$escaped.'["\']/i', $html, $m)) {
            return $m[1] !== '' ? $m[1] : null;
        }

        return null;
    }

    public function extractSpkNilai(string $html): ?string
    {
        return $this->extractNilaiKontrak($html);
    }

    public function extractNilaiKontrak(string $html): ?string
    {
        return $this->extractInputById($html, 'nilaiKontrak_f')
            ?? $this->extractInputValue($html, 'spk.spk_nilai');
    }

    public function extractInputById(string $html, string $id): ?string
    {
        $escaped = preg_quote($id, '/');
        if (preg_match('/id=["\']'.$escaped.'["\'][^>]*value=["\']([^"\']*)["\']/i', $html, $m)) {
            return $m[1] !== '' ? $m[1] : null;
        }

        if (preg_match('/value=["\']([^"\']*)["\'][^>]*id=["\']'.$escaped.'["\']/i', $html, $m)) {
            return $m[1] !== '' ? $m[1] : null;
        }

        return null;
    }

    public function resolveRekananId(string $html, string $penyediaNama, ?string $preferredId = null): ?string
    {
        if ($preferredId) {
            return $preferredId;
        }

        $normalizedTarget = $this->normalizeName($penyediaNama);
        if ($normalizedTarget === '') {
            return null;
        }

        foreach ($this->extractRekananOptions($html) as $option) {
            if ($this->normalizeName($option['label']) === $normalizedTarget) {
                return $option['id'];
            }
        }

        foreach ($this->extractRekananOptions($html) as $option) {
            $label = $this->normalizeName($option['label']);
            if ($label !== '' && (str_contains($label, $normalizedTarget) || str_contains($normalizedTarget, $label))) {
                return $option['id'];
            }
        }

        return null;
    }

    private function firstMatch(string $html, string $pattern): ?string
    {
        return preg_match($pattern, $html, $m) ? $m[1] : null;
    }

    private function normalizeName(string $name): string
    {
        $name = mb_strtoupper(trim($name));
        $name = preg_replace('/\s+/', ' ', $name) ?? $name;
        $name = preg_replace('/[.,\(\)]/', '', $name) ?? $name;

        return $name;
    }
}