<?php

namespace App\Services\Procurement;

class SpseKontrakHtmlParser
{
    /**
     * @return array{sppbj_id: ?string, spk_id: ?string}
     */
    public function extractIdsFromListHtml(string $html): array
    {
        $status = $this->extractKontrakListStatus($html);

        return [
            'sppbj_id' => $status['sppbj_id'],
            'spk_id' => $status['spk_id'],
        ];
    }

    /**
     * @return array{
     *     sppbj_id: ?string,
     *     spk_id: ?string,
     *     pesanan_id: ?string,
     *     sppbj_complete: bool,
     *     spk_complete: bool,
     *     sskk_complete: bool,
     *     spmk_complete: bool,
     *     all_complete: bool,
     * }
     */
    public function extractKontrakListStatus(string $html): array
    {
        $tableHtml = $this->extractElementHtml($html, 'table', 'tblsppbj') ?? $html;

        $sppbjId = $this->firstMatch($tableHtml, '/sppbjId=(\d+)/i')
            ?? $this->firstMatch($tableHtml, '/simpancarapembayaran\?id=(\d+)/i')
            ?? $this->extractSppbjIdFromHtml($tableHtml);
        $spkId = $this->firstMatch($tableHtml, '/spkId=(\d+)/i');
        $pesananId = $this->firstMatch($tableHtml, '/pesananId=(\d+)/i');

        $sppbjComplete = $sppbjId !== null;
        $spkComplete = $spkId !== null;
        $spmkComplete = $pesananId !== null
            || (bool) preg_match('/editspmknonpl\?[^"\']*spkId=\d+/i', $tableHtml);
        $sskkComplete = $spmkComplete
            || (bool) preg_match('/sskk-pl\/(?:cetak|lihat)/i', $tableHtml)
            || (bool) preg_match('/>\s*Sekaligus\s*</i', $tableHtml);

        return [
            'sppbj_id' => $sppbjId,
            'spk_id' => $spkId,
            'pesanan_id' => $pesananId,
            'sppbj_complete' => $sppbjComplete,
            'spk_complete' => $spkComplete,
            'sskk_complete' => $sskkComplete,
            'spmk_complete' => $spmkComplete,
            'all_complete' => $sppbjComplete && $spkComplete && $sskkComplete && $spmkComplete,
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

        $hiddenId = $this->extractHiddenRekananId($html);
        if ($hiddenId !== null) {
            $options[] = ['id' => $hiddenId, 'label' => ''];
        }

        return $options;
    }

    public function extractHiddenValue(string $html, string $name): ?string
    {
        return $this->extractInputValue($html, $name);
    }

    public function extractQueryParam(string $urlOrText, string $param): ?string
    {
        $pattern = '/[?&]'.preg_quote($param, '/').'=(\d+)/i';

        return preg_match($pattern, $urlOrText, $match) ? $match[1] : null;
    }

    public function extractSppbjIdFromHtml(string $html): ?string
    {
        $patterns = [
            '/name=["\']sppbj\.sppbj_id["\'][^>]*value=["\'](\d+)["\']/i',
            '/value=["\'](\d+)["\'][^>]*name=["\']sppbj\.sppbj_id["\']/i',
            '/sppbj-pl\/sppbjppkpl\?[^"\']*sppbjId=(\d+)/i',
            '/spk-pl\/spkpl\?sppbjId=(\d+)/i',
            '/sppbjId=(\d+)/i',
        ];

        foreach ($patterns as $pattern) {
            $id = $this->firstMatch($html, $pattern);
            if ($this->isValidSpseId($id)) {
                return $id;
            }
        }

        return null;
    }

    /**
     * @return array<int, string>
     */
    public function extractSpseUserMessages(string $html): array
    {
        $messages = [];

        $patterns = [
            '/<div[^>]*class=["\'][^"\']*alert-danger[^"\']*["\'][^>]*>(.*?)<\/div>/is',
            '/<div[^>]*class=["\'][^"\']*alert-warning[^"\']*["\'][^>]*>(.*?)<\/div>/is',
            '/<span[^>]*class=["\'][^"\']*error[^"\']*["\'][^>]*>(.*?)<\/span>/is',
        ];

        foreach ($patterns as $pattern) {
            if (! preg_match_all($pattern, $html, $matches, PREG_SET_ORDER)) {
                continue;
            }

            foreach ($matches as $match) {
                $text = trim(html_entity_decode(strip_tags($match[1])));
                if ($text !== '') {
                    $messages[] = $text;
                }
            }
        }

        return array_values(array_unique($messages));
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
        if ($preferredId && $preferredId !== '0') {
            return $preferredId;
        }

        $hiddenId = $this->extractHiddenRekananId($html);
        if ($hiddenId !== null) {
            return $hiddenId;
        }

        $selectedId = $this->extractSelectedRekananId($html);
        if ($selectedId !== null) {
            return $selectedId;
        }

        $labeledOptions = $this->extractLabeledRekananOptions($html);
        if (count($labeledOptions) === 1) {
            return $labeledOptions[0]['id'];
        }

        $normalizedTarget = $this->normalizeName($penyediaNama);
        if ($normalizedTarget === '') {
            return null;
        }

        foreach ($labeledOptions as $option) {
            if ($this->normalizeName($option['label']) === $normalizedTarget) {
                return $option['id'];
            }
        }

        foreach ($labeledOptions as $option) {
            $label = $this->normalizeName($option['label']);
            if ($label !== '' && (str_contains($label, $normalizedTarget) || str_contains($normalizedTarget, $label))) {
                return $option['id'];
            }
        }

        return null;
    }

    public function extractHiddenRekananId(string $html): ?string
    {
        $value = $this->extractHiddenValue($html, 'rekananId');

        return $this->isValidRekananId($value) ? $value : null;
    }

    public function extractSelectedRekananId(string $html): ?string
    {
        if (preg_match(
            '/<select[^>]*name=["\']rekananId["\'][^>]*>.*?<option[^>]*selected[^>]*value=["\'](\d+)["\']/is',
            $html,
            $match,
        )) {
            return $this->isValidRekananId($match[1]) ? $match[1] : null;
        }

        if (preg_match(
            '/<option[^>]*value=["\'](\d+)["\'][^>]*selected[^>]*>.*?<\/select>/is',
            $html,
            $match,
        )) {
            return $this->isValidRekananId($match[1]) ? $match[1] : null;
        }

        return null;
    }

    /**
     * @return array<int, array{id: string, label: string}>
     */
    public function extractLabeledRekananOptions(string $html): array
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
                if (! $this->isValidRekananId($id)) {
                    continue;
                }
                $label = trim(html_entity_decode(strip_tags($match[2])));
                if ($label === '' || mb_strtolower($label) === 'pilih') {
                    continue;
                }
                $options[] = ['id' => $id, 'label' => $label];
            }
        }

        return $options;
    }

    /**
     * @return array<int, string>
     */
    public function listRekananLabels(string $html): array
    {
        return array_values(array_map(
            static fn (array $option): string => $option['label'],
            $this->extractLabeledRekananOptions($html),
        ));
    }

    private function isValidRekananId(?string $id): bool
    {
        return $this->isValidSpseId($id);
    }

    private function isValidSpseId(?string $id): bool
    {
        $id = trim((string) $id);

        return $id !== '' && $id !== '0';
    }

    private function extractElementHtml(string $html, string $tag, string $id): ?string
    {
        $escaped = preg_quote($id, '/');
        if (preg_match('/<'.$tag.'[^>]*id=["\']'.$escaped.'["\'][^>]*>.*?<\/'.$tag.'>/is', $html, $match)) {
            return $match[0];
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