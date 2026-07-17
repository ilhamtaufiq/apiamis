<?php

namespace App\Services\Procurement;

use App\Http\Resources\BerkasResource;
use App\Models\Berkas;
use App\Models\SpseSession;
use Illuminate\Support\Str;

class SpseBerkasImportService
{
    public function __construct(
        private readonly SpseDocumentDownloader $downloader,
    ) {
    }

    /**
     * @param  array<int, array{url: string, jenis_dokumen: string, label?: string}>  $documents
     * @return array{imported: int, failed: int, results: array<int, array<string, mixed>>}
     */
    public function import(SpseSession $session, int $pekerjaanId, array $documents): array
    {
        $imported = 0;
        $failed = 0;
        $results = [];

        foreach ($documents as $index => $document) {
            $url = (string) ($document['url'] ?? '');
            $jenisDokumen = trim((string) ($document['jenis_dokumen'] ?? ''));
            $label = isset($document['label']) ? (string) $document['label'] : null;

            if ($url === '' || $jenisDokumen === '') {
                $failed++;
                $results[] = [
                    'index' => $index,
                    'status' => 'failed',
                    'url' => $url,
                    'reason' => 'url atau jenis_dokumen kosong',
                ];

                continue;
            }

            // Legacy section pages — not file URLs (always 404 on SPSE inaproc).
            if (preg_match('#/nontender/\d+/(pengumumanlelang|beritaacara|dokumenkualifikasi|suratpenawaran|administrasiteknis|dokumenharga|evaluasiteknis|persyaratankualifikasi)/?$#i', $url)) {
                $failed++;
                $results[] = [
                    'index' => $index,
                    'status' => 'failed',
                    'url' => $url,
                    'reason' => 'URL section SPSE (bukan file). Pakai link /dl, /dlsec, viewpdfpl, atau cetak*.',
                ];

                continue;
            }

            $berkas = Berkas::create([
                'pekerjaan_id' => $pekerjaanId,
                'jenis_dokumen' => $jenisDokumen,
            ]);

            try {
                $file = $this->downloader->download($session, $url, $label);
                $storedName = Str::uuid().'-'.$file['filename'];

                $berkas->addMediaFromString($file['body'])
                    ->usingFileName($storedName)
                    ->toMediaCollection('berkas/dokumen');

                $berkas->load('pekerjaan');
                $imported++;
                $results[] = [
                    'index' => $index,
                    'status' => 'imported',
                    'url' => $url,
                    'berkas' => (new BerkasResource($berkas))->resolve(),
                ];
            } catch (\Throwable $e) {
                $berkas->delete();
                $failed++;
                $message = $e->getMessage();
                if (str_contains($message, 'HTTP 404')) {
                    $message = 'SPSE unduh gagal: HTTP 404 (URL tidak ada atau butuh sesi berbeda).';
                }
                $results[] = [
                    'index' => $index,
                    'status' => 'failed',
                    'url' => $url,
                    'reason' => $message,
                ];
            }
        }

        return [
            'imported' => $imported,
            'failed' => $failed,
            'results' => $results,
        ];
    }
}