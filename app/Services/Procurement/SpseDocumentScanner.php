<?php

namespace App\Services\Procurement;

use App\Models\SpseSession;

class SpseDocumentScanner
{
    private const ANCHOR_PATTERN = '/<a[^>]+href=["\']([^"\']+)["\'][^>]*>(.*?)<\/a>/is';

    /** Direct PDF endpoints under /nontender/{id}/ (mirror python downloader.py). */
    private const KNOWN_ENDPOINTS = [
        ['path' => 'pengumumanlelang', 'label' => 'Summary Non Tender', 'doc_type' => 'summary'],
        ['path' => 'beritaacara', 'label' => 'Berita Acara Hasil Pengadaan', 'doc_type' => 'berita_acara'],
        ['path' => 'dokumenkualifikasi', 'label' => 'Dokumen Kualifikasi', 'doc_type' => 'kualifikasi'],
        ['path' => 'suratpenawaran', 'label' => 'Surat Penawaran', 'doc_type' => 'surat_penawaran'],
        ['path' => 'administrasiteknis', 'label' => 'Administrasi dan Teknis', 'doc_type' => 'admin_teknis'],
        ['path' => 'dokumenharga', 'label' => 'Dokumen Harga', 'doc_type' => 'harga'],
        ['path' => 'evaluasiteknis', 'label' => 'Evaluasi Teknis', 'doc_type' => 'evaluasi_teknis'],
        ['path' => 'persyaratankualifikasi', 'label' => 'Persyaratan Kualifikasi Lainnya', 'doc_type' => 'persyaratan_kualifikasi'],
    ];

    public function __construct(
        private readonly SpseHttpClient $httpClient,
    ) {
    }

    /**
     * @return array<int, array{id: string, url: string, label: string, source_page: string, kind: string, doc_type: string}>
     */
    public function scan(SpseSession $session, string $kodePaket, ?string $jenisPaket = null): array
    {
        $kodePaket = trim($kodePaket);
        if ($kodePaket === '') {
            throw new \InvalidArgumentException('kode_paket wajib diisi.');
        }

        if ($jenisPaket === 'tender_seleksi') {
            return $this->scanTender($session, $kodePaket);
        }

        return $this->scanNontender($session, $kodePaket);
    }

    /**
     * Full discovery mirroring python/spse/penawaran main.py discover_all_documents().
     *
     * @return array<int, array{id: string, url: string, label: string, source_page: string, kind: string, doc_type: string}>
     */
    public function scanNontender(SpseSession $session, string $kodePaket): array
    {
        $documents = [];
        $seen = [];
        $referer = '/beranda/nontender';

        $kualUrl = null;
        $evalUrl = null;
        $rincianFollowups = [];

        // 1. Halaman nontender — summary, dl, link kualifikasi/evaluasi
        $nontenderPath = "/nontender/{$kodePaket}";
        $nontenderHtml = $this->tryFetch($session, $nontenderPath, $referer);
        if ($nontenderHtml) {
            foreach ($this->discoverFromNontenderHtml($nontenderHtml) as $doc) {
                $this->pushDocument($session, $documents, $seen, $doc, $nontenderPath);
                if ($doc['doc_type'] === 'admin_teknis' || $doc['doc_type'] === 'harga') {
                    $rincianFollowups[] = $doc;
                }
            }
            $kualUrl = $this->extractFirstHref($nontenderHtml, '/kualifikasinontender\/\d+/i');
            $evalUrl = $this->extractFirstHref($nontenderHtml, '/evaluasinontender\/\d+/i');
        }

        // 2. Halaman penawaran peserta
        $penawaranPath = "/pesertanontender/{$kodePaket}/penawaran";
        $penawaranHtml = $this->tryFetch($session, $penawaranPath, $referer);
        if ($penawaranHtml) {
            foreach ($this->discoverFromPenawaranHtml($penawaranHtml) as $doc) {
                $this->pushDocument($session, $documents, $seen, $doc, $penawaranPath);
                if ($doc['doc_type'] === 'admin_teknis' || $doc['doc_type'] === 'harga') {
                    $rincianFollowups[] = $doc;
                }
            }
            $kualUrl = $kualUrl ?: $this->extractFirstHref($penawaranHtml, '/kualifikasinontender\/\d+/i');
            $evalFromPenawaran = $this->extractFirstHref($penawaranHtml, '/evaluasinontender\/\d+/i');
            if ($evalFromPenawaran) {
                $evalUrl = $evalFromPenawaran;
            }
        }

        // 3. Halaman kualifikasi (ID berbeda dari kode_paket)
        if ($kualUrl) {
            $kualPath = $this->pathFromUrl($kualUrl);
            $kualHtml = $this->tryFetch($session, $kualPath, $referer);
            if ($kualHtml) {
                foreach ($this->discoverFromKualifikasiHtml($kualHtml) as $doc) {
                    $this->pushDocument($session, $documents, $seen, $doc, $kualPath);
                }
            }

            $penawaranId = $this->extractPenawaranIdFromKualifikasiUrl($kualUrl);
            if ($penawaranId) {
                $evalDetailPath = "/evaluasinontender/{$penawaranId}/detail";
                $evalHtml = $this->tryFetch($session, $evalDetailPath, $referer);
                if ($evalHtml) {
                    foreach ($this->discoverFromEvaluasiHtml($evalHtml) as $doc) {
                        $this->pushDocument($session, $documents, $seen, $doc, $evalDetailPath);
                    }
                }
            }
        } elseif ($evalUrl) {
            $evalPath = $this->pathFromUrl($evalUrl);
            $evalHtml = $this->tryFetch($session, $evalPath, $referer);
            if ($evalHtml) {
                foreach ($this->discoverFromEvaluasiHtml($evalHtml) as $doc) {
                    $this->pushDocument($session, $documents, $seen, $doc, $evalPath);
                }
            }
        }

        // 4. Rincian admin teknis / harga — file penyedia per dokumen
        foreach ($this->uniqueRincianFollowups($rincianFollowups) as $followup) {
            $rincianPath = $this->pathFromUrl($followup['url']);
            $rincianHtml = $this->tryFetch($session, $rincianPath, $referer);
            if (! $rincianHtml) {
                continue;
            }
            $source = $followup['doc_type'] === 'admin_teknis' ? 'rincian_adminteknis' : 'rincian_harga';
            foreach ($this->discoverFromRincianHtml($rincianHtml) as $doc) {
                $doc['doc_type'] = $doc['doc_type'] ?: $source;
                $this->pushDocument($session, $documents, $seen, $doc, $rincianPath);
            }
        }

        // 5. Endpoint PDF langsung (fallback seperti downloader.py)
        foreach (self::KNOWN_ENDPOINTS as $endpoint) {
            $path = "/nontender/{$kodePaket}/{$endpoint['path']}";
            $this->pushDocument($session, $documents, $seen, [
                'url' => $path,
                'label' => $endpoint['label'],
                'kind' => 'endpoint',
                'doc_type' => $endpoint['doc_type'],
            ], $path);
        }

        usort($documents, fn (array $a, array $b) => strcmp($a['label'], $b['label']));

        return $documents;
    }

    /**
     * @return array<int, array{id: string, url: string, label: string, source_page: string, kind: string, doc_type: string}>
     */
    private function scanTender(SpseSession $session, string $kodePaket): array
    {
        $documents = [];
        $seen = [];
        $referer = '/home';

        foreach ($this->pagesForTender($kodePaket) as $page) {
            $html = $this->tryFetch($session, $page['path'], $referer);
            if (! $html) {
                continue;
            }
            foreach ($this->extractGenericDocumentLinks($html) as $doc) {
                $this->pushDocument($session, $documents, $seen, $doc, $page['path']);
            }
        }

        usort($documents, fn (array $a, array $b) => strcmp($a['label'], $b['label']));

        return $documents;
    }

    /**
     * @return array<int, array{path: string}>
     */
    public function pagesForTender(string $kodePaket): array
    {
        return [
            ['path' => "/tender/{$kodePaket}"],
            ['path' => "/evaluasi/{$kodePaket}"],
            ['path' => "/peserta/{$kodePaket}/penawaran"],
        ];
    }

    /**
     * @return array<int, array{url: string, label: string, kind: string, doc_type: string}>
     */
    public function discoverFromNontenderHtml(string $html): array
    {
        $docs = [];

        foreach ($this->matchAnchors($html, '/viewpdfpl/i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => $link['label'] !== '' ? $link['label'] : 'Summary Non Tender',
                'kind' => 'generated',
                'doc_type' => 'summary',
            ];
        }

        foreach ($this->matchAnchors($html, '/\/dl\//i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => $link['label'] !== '' ? $link['label'] : $this->labelFromUrl($link['url']),
                'kind' => 'download',
                'doc_type' => 'dl',
            ];
        }

        foreach ($this->matchAnchors($html, '/\/dlsec\//i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => $link['label'] !== '' ? $link['label'] : $this->labelFromUrl($link['url']),
                'kind' => 'download',
                'doc_type' => 'dlsec',
            ];
        }

        return $this->dedupeDocs($docs);
    }

    /**
     * @return array<int, array{url: string, label: string, kind: string, doc_type: string}>
     */
    public function discoverFromPenawaranHtml(string $html): array
    {
        $docs = [];

        foreach ($this->matchAnchors($html, '/cetaksuratpenawaranpeserta|cetak/i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => $link['label'] !== '' ? $link['label'] : 'Surat Penawaran',
                'kind' => 'generated',
                'doc_type' => 'surat_penawaran',
            ];
        }

        foreach ($this->matchAnchors($html, '/rincian_adminteknis/i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => 'Administrasi dan Teknis',
                'kind' => 'html_page',
                'doc_type' => 'admin_teknis',
            ];
        }

        foreach ($this->matchAnchors($html, '/rincian_penawaran/i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => 'Harga',
                'kind' => 'html_page',
                'doc_type' => 'harga',
            ];
        }

        foreach ($this->matchAnchors($html, '/\/dl\//i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => $link['label'] !== '' ? $link['label'] : $this->labelFromUrl($link['url']),
                'kind' => 'download',
                'doc_type' => 'dl',
            ];
        }

        return $this->dedupeDocs($docs);
    }

    /**
     * @return array<int, array{url: string, label: string, kind: string, doc_type: string}>
     */
    public function discoverFromKualifikasiHtml(string $html): array
    {
        $docs = [];

        foreach ($this->matchAnchors($html, '/cetakkualifikasipl/i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => 'Dokumen Kualifikasi',
                'kind' => 'generated',
                'doc_type' => 'kualifikasi',
            ];
        }

        foreach ($this->matchAnchors($html, '/\/dl\//i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => $link['label'] !== '' ? $link['label'] : $this->labelFromUrl($link['url']),
                'kind' => 'download',
                'doc_type' => 'dl_kualifikasi',
            ];
        }

        return $this->dedupeDocs($docs);
    }

    /**
     * @return array<int, array{url: string, label: string, kind: string, doc_type: string}>
     */
    public function discoverFromEvaluasiHtml(string $html): array
    {
        $docs = [];

        foreach ($this->matchAnchors($html, '/\/dlsec\/|\/dl\//i') as $link) {
            if ($link['label'] === '') {
                continue;
            }
            $docs[] = [
                'url' => $link['url'],
                'label' => $link['label'],
                'kind' => 'download',
                'doc_type' => 'dlsec',
            ];
        }

        return $this->dedupeDocs($docs);
    }

    /**
     * @return array<int, array{url: string, label: string, kind: string, doc_type: string}>
     */
    public function discoverFromRincianHtml(string $html): array
    {
        $docs = [];

        foreach ($this->matchAnchors($html, '/\/dl\/|\/dlsec\/|dokumennontender/i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => $link['label'] !== '' ? $link['label'] : $this->labelFromUrl($link['url']),
                'kind' => 'download',
                'doc_type' => 'dl_rincian',
            ];
        }

        return $this->dedupeDocs($docs);
    }

    /**
     * @return array<int, array{url: string, label: string, kind: string, doc_type: string}>
     */
    public function extractGenericDocumentLinks(string $html): array
    {
        $docs = [];

        foreach ($this->matchAnchors($html, '/viewpdfpl|\/dl\/|\/dlsec\/|cetak|download|dokumennontender/i') as $link) {
            $docs[] = [
                'url' => $link['url'],
                'label' => $link['label'] !== '' ? $link['label'] : $this->labelFromUrl($link['url']),
                'kind' => $this->classifyKind($link['url']),
                'doc_type' => 'generic',
            ];
        }

        return $this->dedupeDocs($docs);
    }

    /**
     * Backward-compatible helper used in tests.
     *
     * @return array<int, array{url: string, label: string}>
     */
    public function extractLinksFromHtml(string $html): array
    {
        return array_map(
            fn (array $doc) => ['url' => $doc['url'], 'label' => $doc['label']],
            $this->extractGenericDocumentLinks($html),
        );
    }

    /**
     * @return array<int, array{path: string, referer?: string}>
     */
    public function pagesForJenis(?string $jenisPaket, string $kodePaket): array
    {
        if ($jenisPaket === 'tender_seleksi') {
            return array_map(
                fn (array $p) => ['path' => $p['path'], 'referer' => '/home'],
                $this->pagesForTender($kodePaket),
            );
        }

        return [
            ['path' => "/nontender/{$kodePaket}", 'referer' => '/beranda/nontender'],
            ['path' => "/pesertanontender/{$kodePaket}/penawaran", 'referer' => '/beranda/nontender'],
        ];
    }

    /**
     * @param  array<int, array{url: string, label: string, kind: string, doc_type: string}>  $docs
     * @param  array<string, true>  $seen
     * @param  array{url: string, label: string, kind: string, doc_type: string}  $doc
     */
    private function pushDocument(SpseSession $session, array &$docs, array &$seen, array $doc, string $sourcePage): void
    {
        $absolute = $this->httpClient->absoluteUrl($session, $doc['url']);

        $id = md5($absolute);
        if (isset($seen[$id])) {
            return;
        }
        $seen[$id] = true;

        $docs[] = [
            'id' => $id,
            'url' => $absolute,
            'label' => mb_substr($doc['label'], 0, 255),
            'source_page' => $sourcePage,
            'kind' => $doc['kind'],
            'doc_type' => $doc['doc_type'],
        ];
    }

    private function tryFetch(SpseSession $session, string $path, ?string $referer = null): ?string
    {
        try {
            return $this->httpClient->fetchPage($session, $path, $referer);
        } catch (\Throwable) {
            return null;
        }
    }

    /**
     * @return array<int, array{url: string, label: string}>
     */
    private function matchAnchors(string $html, string $hrefPattern): array
    {
        $links = [];
        if (! preg_match_all(self::ANCHOR_PATTERN, $html, $matches, PREG_SET_ORDER)) {
            return [];
        }

        foreach ($matches as $match) {
            $href = html_entity_decode(trim($match[1]));
            if ($href === '' || ! preg_match($hrefPattern, $href)) {
                continue;
            }
            $links[] = [
                'url' => $href,
                'label' => $this->normalizeLabel($match[2]),
            ];
        }

        return $links;
    }

    private function extractFirstHref(string $html, string $hrefPattern): ?string
    {
        if (! preg_match_all(self::ANCHOR_PATTERN, $html, $matches, PREG_SET_ORDER)) {
            return null;
        }

        foreach ($matches as $match) {
            $href = html_entity_decode(trim($match[1]));
            if ($href !== '' && preg_match($hrefPattern, $href)) {
                return $href;
            }
        }

        return null;
    }

    private function extractPenawaranIdFromKualifikasiUrl(string $url): ?string
    {
        if (preg_match('/\/kualifikasinontender\/(\d+)\//i', $url, $matches)) {
            return $matches[1];
        }

        return null;
    }

    private function pathFromUrl(string $url): string
    {
        $path = parse_url($url, PHP_URL_PATH);

        return $path ? (string) $path : $url;
    }

    private function classifyKind(string $url): string
    {
        if (preg_match('/\/(dl|dlsec)\//i', $url)) {
            return 'download';
        }
        if (preg_match('/viewpdfpl|cetak/i', $url)) {
            return 'generated';
        }

        return 'download';
    }

    private function normalizeLabel(string $html): string
    {
        $text = trim(preg_replace('/\s+/u', ' ', strip_tags($html)) ?? '');

        return mb_substr($text, 0, 255);
    }

    private function labelFromUrl(string $url): string
    {
        $path = parse_url($url, PHP_URL_PATH) ?: $url;
        $basename = basename((string) $path);

        return $basename !== '' && $basename !== '/' ? urldecode($basename) : 'Dokumen SPSE';
    }

    /**
     * @param  array<int, array{url: string, label: string, kind: string, doc_type: string}>  $docs
     * @return array<int, array{url: string, label: string, kind: string, doc_type: string}>
     */
    private function dedupeDocs(array $docs): array
    {
        $unique = [];
        foreach ($docs as $doc) {
            $key = $doc['url'];
            if (! isset($unique[$key]) || strlen($doc['label']) > strlen($unique[$key]['label'])) {
                $unique[$key] = $doc;
            }
        }

        return array_values($unique);
    }

    /**
     * @param  array<int, array{url: string, label: string, kind: string, doc_type: string}>  $followups
     * @return array<int, array{url: string, label: string, kind: string, doc_type: string}>
     */
    private function uniqueRincianFollowups(array $followups): array
    {
        return $this->dedupeDocs($followups);
    }
}