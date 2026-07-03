<?php

namespace App\Services\Procurement;

use App\Models\SpseSession;

class SpseDocumentDownloader
{
    public function __construct(
        private readonly SpseHttpClient $httpClient,
    ) {
    }

    /**
     * @return array{body: string, filename: string, mime_type: string}
     */
    public function download(SpseSession $session, string $url, ?string $label = null): array
    {
        $result = $this->httpClient->downloadBinary($session, $url);
        $body = $result['body'];

        if ($body === '') {
            throw new \RuntimeException('File SPSE kosong.');
        }

        $mimeType = $this->resolveMimeType($result['content_type'], $body);
        $filename = $this->resolveFilename(
            $result['content_disposition'],
            $result['final_url'],
            $label,
            $mimeType,
        );

        return [
            'body' => $body,
            'filename' => $filename,
            'mime_type' => $mimeType,
        ];
    }

    public function resolveFilename(
        ?string $contentDisposition,
        string $url,
        ?string $label,
        string $mimeType,
    ): string {
        if ($contentDisposition && preg_match('/filename\*?=(?:UTF-8\'\')?"?([^";]+)"?/i', $contentDisposition, $matches)) {
            $fromHeader = trim(urldecode($matches[1]));
            if ($fromHeader !== '') {
                return $this->sanitizeFilename($fromHeader);
            }
        }

        if ($label) {
            $fromLabel = $this->sanitizeFilename($label);
            if (! $this->hasExtension($fromLabel)) {
                $fromLabel .= $this->extensionForMime($mimeType);
            }
            if ($fromLabel !== '') {
                return $fromLabel;
            }
        }

        $path = parse_url($url, PHP_URL_PATH) ?: '';
        $basename = basename((string) $path);
        if ($basename !== '' && $basename !== '/') {
            return $this->sanitizeFilename(urldecode($basename));
        }

        return 'dokumen-spse'.$this->extensionForMime($mimeType);
    }

    public function resolveMimeType(?string $contentType, string $body): string
    {
        if ($contentType) {
            $mime = strtolower(trim(explode(';', $contentType)[0]));
            if ($mime !== '' && $mime !== 'application/octet-stream') {
                return $mime;
            }
        }

        if (str_starts_with($body, '%PDF')) {
            return 'application/pdf';
        }

        return 'application/octet-stream';
    }

    private function extensionForMime(string $mimeType): string
    {
        return match ($mimeType) {
            'application/pdf' => '.pdf',
            'application/zip', 'application/x-zip-compressed' => '.zip',
            'image/jpeg' => '.jpg',
            'image/png' => '.png',
            default => '.bin',
        };
    }

    private function hasExtension(string $filename): bool
    {
        return (bool) preg_match('/\.[a-z0-9]{2,5}$/i', $filename);
    }

    private function sanitizeFilename(string $filename): string
    {
        $filename = preg_replace('/[\\\\\/:*?"<>|]+/', '-', $filename) ?? $filename;
        $filename = trim($filename, " \t\n\r\0\x0B.-");

        return mb_substr($filename, 0, 200);
    }
}