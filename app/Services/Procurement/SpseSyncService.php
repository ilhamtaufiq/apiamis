<?php

namespace App\Services\Procurement;

use App\Models\ProcurementStagingPaket;
use App\Models\ProcurementSyncRun;
use App\Models\SpseSession;

class SpseSyncService
{
    private const MAX_PAGES = 50;

    private const SOURCES = [
        'pengadaan_langsung' => [
            'endpoint' => '/dt/paket-ppk-pl',
            'referer' => '/beranda/nontender',
        ],
        'tender_seleksi' => [
            'endpoint' => '/dt/paket-ppk',
            'referer' => '/home',
        ],
    ];

    public function __construct(
        private readonly SpseHttpClient $httpClient,
        private readonly ProcurementMatchingService $matchingService,
    ) {
    }

    public function sync(SpseSession $session, int $userId, int $pageLength = 100): ProcurementSyncRun
    {
        $run = ProcurementSyncRun::create([
            'user_id' => $userId,
            'status' => 'running',
            'started_at' => now(),
        ]);

        $totalItems = 0;
        $matchedCount = 0;
        $errors = [];

        try {
            foreach (self::SOURCES as $jenis => $source) {
                $seen = [];

                try {
                    $this->fetchAllPages(
                        $session,
                        $source['endpoint'],
                        $pageLength,
                        $source['referer'],
                        function (array $row) use ($run, $jenis, &$seen, &$totalItems, &$matchedCount, &$errors): void {
                            $kode = trim((string) ($row[0] ?? ''));
                            // Baris tanpa kode tidak bisa dicocokkan; baris ganda muncul bila data bergeser antar halaman.
                            if ($kode === '' || isset($seen[$kode])) {
                                return;
                            }
                            $seen[$kode] = true;

                            try {
                                $staging = $this->storeRow($run, $jenis, $row);
                                $staging = $this->matchingService->matchStaging($staging);
                            } catch (\Throwable $e) {
                                $errors[] = $jenis.' ['.$kode.']: '.$e->getMessage();

                                return;
                            }

                            $totalItems++;
                            if ($staging->match_status !== 'unmatched') {
                                $matchedCount++;
                            }
                        },
                    );
                } catch (SpseSessionExpiredException $e) {
                    $session->update(['is_active' => false]);
                    $errors[] = $jenis.': '.$e->getMessage();
                    break;
                } catch (\Throwable $e) {
                    $errors[] = $jenis.': '.$e->getMessage();
                }
            }
        } finally {
            $errors = array_slice($errors, 0, 50);
            $run->update([
                'status' => $errors === [] ? 'completed' : ($totalItems > 0 ? 'partial' : 'failed'),
                'item_count' => $totalItems,
                'matched_count' => $matchedCount,
                'error_log' => $errors === [] ? null : implode("\n", $errors),
                'finished_at' => now(),
            ]);
        }

        return $run->fresh(['stagingPakets.pekerjaan', 'stagingPakets.kontrak']);
    }

    /**
     * Ambil semua halaman dan serahkan tiap baris ke $onRow segera,
     * sehingga halaman yang sudah terambil tidak hilang bila halaman berikutnya gagal.
     *
     * @param  callable(array<int, mixed>): void  $onRow
     */
    private function fetchAllPages(
        SpseSession $session,
        string $endpoint,
        int $pageLength,
        string $refererPath,
        callable $onRow,
    ): void {
        $start = 0;
        $pageLength = max(1, min($pageLength, 500));

        for ($draw = 1; $draw <= self::MAX_PAGES; $draw++) {
            $json = $this->httpClient->fetchDataTable(
                $session,
                $endpoint,
                status: 1,
                start: $start,
                length: $pageLength,
                draw: $draw,
                refererPath: $refererPath,
            );

            $rows = $json['data'] ?? [];
            if (! is_array($rows) || $rows === []) {
                return;
            }

            foreach ($rows as $row) {
                if (is_array($row)) {
                    $onRow($row);
                }
            }

            $start += count($rows);
            $total = (int) ($json['recordsFiltered'] ?? $json['recordsTotal'] ?? 0);

            if (count($rows) < $pageLength || ($total > 0 && $start >= $total)) {
                return;
            }
        }

        throw new \RuntimeException('Sync dihentikan: melebihi batas '.self::MAX_PAGES.' halaman, data mungkin belum lengkap.');
    }

    /**
     * @param  array<int, mixed>  $row
     */
    private function storeRow(ProcurementSyncRun $run, string $jenis, array $row): ProcurementStagingPaket
    {
        return ProcurementStagingPaket::create([
            'sync_run_id' => $run->id,
            'sumber' => 'spse',
            'jenis_paket' => $jenis,
            'kode_paket' => mb_substr(trim((string) ($row[0] ?? '')), 0, 32),
            'nama_paket' => mb_substr($this->cleanText($row[1] ?? ''), 0, 500),
            'status_paket' => isset($row[2]) ? mb_substr($this->cleanText($row[2]), 0, 128) : null,
            'metode_pengadaan' => isset($row[5]) ? mb_substr($this->cleanText($row[5]), 0, 128) : null,
            'raw_row' => $row,
            'fetched_at' => now(),
        ]);
    }

    private function cleanText(mixed $value): string
    {
        if (is_array($value)) {
            $value = implode(' ', array_filter(array_map('strval', array_filter($value, 'is_scalar'))));
        }

        $text = html_entity_decode(strip_tags((string) $value), ENT_QUOTES | ENT_HTML5, 'UTF-8');

        return trim(preg_replace('/\s+/u', ' ', $text) ?? '');
    }
}
