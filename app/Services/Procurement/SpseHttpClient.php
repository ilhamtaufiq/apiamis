<?php

namespace App\Services\Procurement;

use App\Models\SpseSession;
use Illuminate\Http\Client\ConnectionException;
use Illuminate\Support\Facades\Http;

class SpseHttpClient
{
    public function __construct(
        private readonly SpseCookieParser $cookieParser,
    ) {
    }

    public function baseUrl(?SpseSession $session = null): string
    {
        $slug = $session?->lpse_slug ?: config('services.spse.lpse_slug', 'cianjurkab');

        return rtrim(config('services.spse.base_url', 'https://spse.inaproc.id'), '/').'/'.$slug;
    }

    public function validateSession(SpseSession $session): bool
    {
        try {
            $response = $this->request($session)->get($this->baseUrl($session).'/home');

            if (! $response->successful()) {
                return false;
            }

            $body = $response->body();

            return ! str_contains(strtolower($body), 'loginctr')
                && ! str_contains(strtolower($body), 'login ctr');
        } catch (ConnectionException) {
            return false;
        }
    }

    /**
     * @return array{draw: string, recordsTotal: int, recordsFiltered: int, data: array<int, array<int, mixed>>}
     */
    public function fetchDataTable(
        SpseSession $session,
        string $endpoint,
        int $status = 1,
        int $start = 0,
        int $length = 100,
        int $draw = 1,
        ?string $refererPath = null,
    ): array {
        $refererPath = $refererPath ?: $this->defaultRefererForEndpoint($endpoint);
        $token = $this->resolveAuthenticityToken($session, $refererPath);
        $url = $this->baseUrl($session).$endpoint.'?status='.$status;
        $referer = $this->baseUrl($session).$refererPath;
        $body = $this->buildDataTablesBody($draw, $start, $length, $token);

        $response = $this->request($session)
            ->asForm()
            ->withHeaders([
                'Referer' => $referer,
                'X-Requested-With' => 'XMLHttpRequest',
            ])
            ->post($url, $body);

        if (! $response->successful()) {
            throw new \RuntimeException('SPSE DataTable gagal: HTTP '.$response->status());
        }

        $json = $response->json();
        if (! is_array($json) || ! isset($json['data']) || ! is_array($json['data'])) {
            throw new \RuntimeException('Response SPSE tidak valid (bukan DataTables JSON).');
        }

        return $json;
    }

    public function resolveAuthenticityToken(SpseSession $session, ?string $refererPath = null): string
    {
        $fromCookie = $this->extractTokenFromSpseSessionCookie($session);
        if ($fromCookie) {
            return $fromCookie;
        }

        $pages = array_values(array_unique(array_filter([
            $refererPath ? $this->baseUrl($session).$refererPath : null,
            $this->baseUrl($session).'/beranda/nontender',
            $this->baseUrl($session).'/home',
        ])));

        foreach ($pages as $pageUrl) {
            $response = $this->request($session)->get($pageUrl);
            if (! $response->successful()) {
                continue;
            }

            $token = $this->extractTokenFromHtml($response->body());
            if ($token) {
                return $token;
            }
        }

        throw new \RuntimeException('Tidak dapat mengambil authenticityToken dari SPSE.');
    }

    public function extractTokenFromSpseSessionCookie(SpseSession $session): ?string
    {
        foreach ($session->encrypted_cookies ?? [] as $cookie) {
            if (strtoupper((string) ($cookie['name'] ?? '')) !== 'SPSE_SESSION') {
                continue;
            }

            $value = (string) ($cookie['value'] ?? '');
            if (preg_match('/___AT=([^&]+)/', $value, $matches)) {
                return $matches[1];
            }
        }

        return null;
    }

    public function extractTokenFromHtml(string $html): ?string
    {
        $patterns = [
            '/d\.authenticityToken\s*=\s*[\'"]([^\'"]+)[\'"]/i',
            '/name=["\']authenticityToken["\']\s+value=["\']([^"\']+)["\']/i',
            '/name=["\']_csrf["\']\s+value=["\']([^"\']+)["\']/i',
            '/authenticityToken["\']\s*:\s*["\']([^"\']+)["\']/i',
        ];

        foreach ($patterns as $pattern) {
            if (preg_match($pattern, $html, $matches)) {
                return $matches[1];
            }
        }

        return null;
    }

    public function defaultRefererForEndpoint(string $endpoint): string
    {
        return str_contains($endpoint, 'paket-ppk-pl')
            ? '/beranda/nontender'
            : '/home';
    }

    /**
     * @return array<string, string>
     */
    public function buildDataTablesBody(int $draw, int $start, int $length, string $authenticityToken): array
    {
        $body = [
            'draw' => (string) $draw,
            'start' => (string) $start,
            'length' => (string) $length,
            'search[value]' => '',
            'search[regex]' => 'false',
            'order[0][column]' => '0',
            'order[0][dir]' => 'desc',
            'authenticityToken' => $authenticityToken,
        ];

        for ($i = 0; $i < 5; $i++) {
            $body["columns[{$i}][data]"] = (string) $i;
            $body["columns[{$i}][name]"] = '';
            $body["columns[{$i}][searchable]"] = 'true';
            $body["columns[{$i}][orderable]"] = $i <= 1 ? 'true' : 'false';
            $body["columns[{$i}][search][value]"] = '';
            $body["columns[{$i}][search][regex]"] = 'false';
        }

        return $body;
    }

    public function absoluteUrl(SpseSession $session, string $urlOrPath): string
    {
        if (str_starts_with($urlOrPath, 'http://') || str_starts_with($urlOrPath, 'https://')) {
            return $urlOrPath;
        }

        $slug = $session->lpse_slug ?: config('services.spse.lpse_slug', 'cianjurkab');
        $root = rtrim(config('services.spse.base_url', 'https://spse.inaproc.id'), '/');
        $path = str_starts_with($urlOrPath, '/') ? $urlOrPath : '/'.$urlOrPath;

        // /cianjurkab/dl/... — href sudah menyertakan slug LPSE
        if (preg_match('#^/'.$slug.'(/|$)#', $path)) {
            return $root.$path;
        }

        return $this->baseUrl($session).$path;
    }

    public function fetchPage(SpseSession $session, string $path, ?string $refererPath = null): string
    {
        $url = $this->absoluteUrl($session, $path);
        $headers = [];
        if ($refererPath) {
            $headers['Referer'] = $this->absoluteUrl($session, $refererPath);
        }

        $response = $this->pageRequest($session)
            ->withHeaders($headers)
            ->get($url);

        if (! $response->successful()) {
            throw new \RuntimeException('SPSE halaman gagal: HTTP '.$response->status().' ('.$path.')');
        }

        return $response->body();
    }

    /**
     * @return array{body: string, content_type: ?string, content_disposition: ?string, final_url: string}
     */
    /**
     * @param  array<string, string>  $fields
     * @return array{status: int, body: string, headers: array<string, string>, location: ?string}
     */
    /**
     * @return array{status: int, body: string, headers: array<string, string>, location: ?string}
     */
    public function postAction(
        SpseSession $session,
        string $urlOrPath,
        ?string $refererPath = null,
    ): array {
        $url = $this->absoluteUrl($session, $urlOrPath);
        $headers = [];
        if ($refererPath) {
            $headers['Referer'] = $this->absoluteUrl($session, $refererPath);
        }

        $response = $this->baseRequest($session)
            ->withHeaders($headers)
            ->withOptions(['allow_redirects' => false])
            ->post($url);

        return [
            'status' => $response->status(),
            'body' => $response->body(),
            'headers' => $response->headers(),
            'location' => $response->header('Location'),
        ];
    }

    public function postMultipart(
        SpseSession $session,
        string $urlOrPath,
        array $fields,
        ?string $refererPath = null,
    ): array {
        $url = $this->absoluteUrl($session, $urlOrPath);
        $multipart = [];
        foreach ($fields as $name => $value) {
            $multipart[] = [
                'name' => $name,
                'contents' => (string) $value,
            ];
        }

        $headers = [];
        if ($refererPath) {
            $headers['Referer'] = $this->absoluteUrl($session, $refererPath);
        }

        $response = $this->baseRequest($session)
            ->withHeaders($headers)
            ->asMultipart()
            ->withOptions(['allow_redirects' => false])
            ->post($url, $multipart);

        $location = $response->header('Location');

        return [
            'status' => $response->status(),
            'body' => $response->body(),
            'headers' => $response->headers(),
            'location' => $location,
        ];
    }

    public function downloadBinary(SpseSession $session, string $urlOrPath): array
    {
        $url = $this->absoluteUrl($session, $urlOrPath);

        $response = $this->pageRequest($session)
            ->withOptions(['allow_redirects' => ['max' => 8, 'track_redirects' => true]])
            ->get($url);

        if (! $response->successful()) {
            throw new \RuntimeException('SPSE unduh gagal: HTTP '.$response->status());
        }

        return [
            'body' => $response->body(),
            'content_type' => $response->header('Content-Type'),
            'content_disposition' => $response->header('Content-Disposition'),
            'final_url' => $url,
        ];
    }

    /**
     * Lightweight check before listing a URL as importable.
     * Rejects 404/HTML login pages so legacy /nontender/{id}/{section} paths
     * are not offered to users (they fail with the same HTTP 404 on every import).
     */
    public function isDownloadableBinary(SpseSession $session, string $urlOrPath, ?string $refererPath = null): bool
    {
        $url = $this->absoluteUrl($session, $urlOrPath);
        $headers = [];
        if ($refererPath) {
            $headers['Referer'] = $this->absoluteUrl($session, $refererPath);
        }

        $response = $this->pageRequest($session)
            ->withHeaders($headers)
            ->withOptions(['allow_redirects' => ['max' => 5]])
            ->get($url);

        if (! $response->successful()) {
            return false;
        }

        $contentType = strtolower((string) $response->header('Content-Type'));
        $disposition = strtolower((string) $response->header('Content-Disposition'));
        $body = $response->body();

        if (str_starts_with($body, '%PDF')) {
            return true;
        }

        if (str_contains($disposition, 'attachment') || str_contains($disposition, 'filename=')) {
            return true;
        }

        if (
            str_contains($contentType, 'pdf')
            || str_contains($contentType, 'zip')
            || str_contains($contentType, 'octet-stream')
            || str_contains($contentType, 'msword')
            || str_contains($contentType, 'officedocument')
        ) {
            return true;
        }

        // HTML error/login pages must not be treated as downloadable documents.
        if (str_contains($contentType, 'text/html') || str_contains($contentType, 'text/plain')) {
            return false;
        }

        return $body !== '';
    }

    private function request(SpseSession $session)
    {
        return $this->baseRequest($session)
            ->withHeaders([
                'Accept' => 'application/json, text/javascript, */*; q=0.01',
            ]);
    }

    private function pageRequest(SpseSession $session)
    {
        return $this->baseRequest($session)
            ->withHeaders([
                'Accept' => 'text/html,application/xhtml+xml,application/pdf,*/*;q=0.8',
            ]);
    }

    private function baseRequest(SpseSession $session)
    {
        $cookies = $session->encrypted_cookies ?? [];
        $header = $this->cookieParser->toHeader($cookies);

        return Http::timeout(120)
            ->withHeaders([
                'User-Agent' => 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
                'Cookie' => $header,
            ]);
    }
}