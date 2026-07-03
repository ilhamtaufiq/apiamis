<?php

namespace App\Services\Procurement;

use App\Models\SpseSession;
use ZipArchive;

class SpseDocumentZipService
{
    public function __construct(
        private readonly SpseDocumentDownloader $downloader,
    ) {
    }

    /**
     * @param  array<int, array{url: string, label?: string}>  $documents
     * @return array{path: string, filename: string, count: int, failed: int, failed_details: array<int, array<string, string>>}
     */
    public function buildZip(SpseSession $session, string $kodePaket, array $documents): array
    {
        $tempFile = tempnam(sys_get_temp_dir(), 'spse_zip');
        if ($tempFile === false) {
            throw new \RuntimeException('Gagal menyiapkan file sementara.');
        }

        $zip = new ZipArchive();
        if ($zip->open($tempFile, ZipArchive::CREATE | ZipArchive::OVERWRITE) !== true) {
            throw new \RuntimeException('Gagal membuat arsip ZIP.');
        }

        $added = 0;
        $failedDetails = [];
        $usedNames = [];

        foreach ($documents as $index => $document) {
            $url = (string) ($document['url'] ?? '');
            $label = isset($document['label']) ? (string) $document['label'] : null;

            if ($url === '') {
                $failedDetails[] = [
                    'url' => $url,
                    'label' => $label ?? '',
                    'reason' => 'url kosong',
                ];

                continue;
            }

            try {
                $file = $this->downloader->download($session, $url, $label);
                $entryName = $this->uniqueEntryName($usedNames, $index + 1, $file['filename']);
                $zip->addFromString($entryName, $file['body']);
                $usedNames[$entryName] = true;
                $added++;
            } catch (\Throwable $e) {
                $failedDetails[] = [
                    'url' => $url,
                    'label' => $label ?? '',
                    'reason' => $e->getMessage(),
                ];
            }
        }

        if ($failedDetails !== []) {
            $zip->addFromString(
                '_gagal.json',
                json_encode($failedDetails, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE) ?: '[]',
            );
        }

        $zip->close();

        $safeKode = preg_replace('/[\\\\\/:*?"<>|]+/', '_', $kodePaket) ?: 'paket';

        return [
            'path' => $tempFile,
            'filename' => "spse_{$safeKode}.zip",
            'count' => $added,
            'failed' => count($failedDetails),
            'failed_details' => $failedDetails,
        ];
    }

    /**
     * @param  array<string, true>  $usedNames
     */
    public function uniqueEntryName(array &$usedNames, int $index, string $filename): string
    {
        $filename = trim($filename) !== '' ? $filename : 'dokumen.pdf';
        $filename = preg_replace('/[\\\\\/:*?"<>|]+/', '-', $filename) ?? $filename;
        $candidate = sprintf('%02d_%s', $index, $filename);

        if (! isset($usedNames[$candidate])) {
            return $candidate;
        }

        $suffix = 2;
        $pathInfo = pathinfo($filename);
        $base = $pathInfo['filename'] ?? $filename;
        $ext = isset($pathInfo['extension']) ? '.'.$pathInfo['extension'] : '';

        do {
            $candidate = sprintf('%02d_%s_%d%s', $index, $base, $suffix, $ext);
            $suffix++;
        } while (isset($usedNames[$candidate]));

        return $candidate;
    }
}