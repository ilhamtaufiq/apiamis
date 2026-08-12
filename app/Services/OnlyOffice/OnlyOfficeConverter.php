<?php

namespace App\Services\OnlyOffice;

use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Str;
use Spatie\MediaLibrary\MediaCollections\Models\Media;

class OnlyOfficeConverter
{
    private const CONVERTIBLE_EXTENSIONS = [
        'doc', 'docx', 'odt', 'rtf', 'txt',
        'xls', 'xlsx', 'ods', 'csv',
        'ppt', 'pptx', 'odp',
        'jpg', 'jpeg', 'png', 'gif', 'bmp', 'webp',
    ];

    public function __construct(
        private readonly OnlyOfficeService $onlyOffice,
    ) {}

    public function convertMediaToPdf(Media $media): ?string
    {
        if (! $this->onlyOffice->isEnabled()) {
            return null;
        }

        $extension = strtolower($media->extension ?: pathinfo($media->file_name, PATHINFO_EXTENSION));

        if ($extension === 'pdf') {
            return $media->getPath();
        }

        if (! in_array($extension, self::CONVERTIBLE_EXTENSIONS, true)) {
            return null;
        }

        $downloadUrl = OnlyOfficeDownloadToken::buildDownloadUrl($media->id);

        return $this->convertRemoteFileToPdf(
            $downloadUrl,
            $extension,
            $this->onlyOffice->buildDocumentKey($media),
            $media->file_name,
        );
    }

    public function convertRemoteFileToPdf(string $fileUrl, string $fileType, string $key, string $title): ?string
    {
        $documentServerUrl = rtrim((string) config('onlyoffice.document_server_url'), '/');

        if ($documentServerUrl === '') {
            return null;
        }

        $payload = [
            'async' => true,
            'filetype' => strtolower($fileType),
            'key' => $key,
            'outputtype' => 'pdf',
            'title' => $title,
            'url' => $fileUrl,
        ];

        $converterUrl = $documentServerUrl.'/converter?shardkey='.urlencode($key);
        $result = $this->requestConversion($converterUrl, $payload);

        if (! $result || empty($result['fileUrl'])) {
            Log::warning('ONLYOFFICE PDF conversion failed', [
                'title' => $title,
                'filetype' => $fileType,
                'result' => $result,
            ]);

            return null;
        }

        return $this->downloadConvertedPdf((string) $result['fileUrl']);
    }

    private function requestConversion(string $converterUrl, array $payload): ?array
    {
        $jwtSecret = (string) config('onlyoffice.jwt_secret');
        $body = $jwtSecret !== ''
            ? ['token' => OnlyOfficeJwt::encode($payload, $jwtSecret)]
            : $payload;

        // Per-attempt timeout reduced to 10 s to bound total wall time to ~8 min.
        for ($attempt = 0; $attempt < 40; $attempt++) {
            try {
                $response = Http::timeout(10)->post($converterUrl, $body);
            } catch (\Throwable $exception) {
                Log::error('ONLYOFFICE converter request failed', [
                    'message' => $exception->getMessage(),
                ]);

                return null;
            }

            if (! $response->successful()) {
                Log::warning('ONLYOFFICE converter HTTP error', [
                    'status' => $response->status(),
                    'body' => $response->body(),
                ]);

                return null;
            }

            $result = $response->json();

            if (! is_array($result)) {
                return null;
            }

            if (! empty($result['error'])) {
                Log::warning('ONLYOFFICE converter returned error', ['result' => $result]);

                return null;
            }

            if (($result['endConvert'] ?? false) === true) {
                return $result;
            }

            usleep(500_000);
        }

        return null;
    }

    private function downloadConvertedPdf(string $fileUrl): ?string
    {
        // SSRF guard: converted PDF URL must resolve to the Document Server host.
        $dsHost = parse_url((string) config('onlyoffice.document_server_url'), PHP_URL_HOST);
        if ($dsHost && ! empty($fileUrl)) {
            $urlHost = parse_url($fileUrl, PHP_URL_HOST);
            if (! $urlHost || strcasecmp($dsHost, $urlHost) !== 0) {
                Log::warning('ONLYOFFICE converter rejected: host mismatch', ['url' => $fileUrl]);

                return null;
            }
        }

        try {
            $response = Http::timeout(10)->get($fileUrl);

            if (! $response->successful()) {
                return null;
            }

            $outputDir = storage_path('app/temp-pdf');

            if (! file_exists($outputDir)) {
                mkdir($outputDir, 0775, true);
            }

            // Cleanup stale temp files older than 24 h to prevent disk bloat.
            foreach (glob("$outputDir/*.pdf") as $old) {
                if (time() - filemtime($old) > DAY_SECONDS) {
                    @unlink($old);
                }
            }

            $outputPath = $outputDir.'/'.Str::uuid().'.pdf';
            file_put_contents($outputPath, $response->body());

            return file_exists($outputPath) ? $outputPath : null;
        } catch (\Throwable $exception) {
            Log::error('ONLYOFFICE converted PDF download failed', [
                'message' => $exception->getMessage(),
            ]);

            return null;
        }
    }
}