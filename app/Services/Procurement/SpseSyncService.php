<?php

namespace App\Services\Procurement;

use App\Models\ProcurementStagingPaket;
use App\Models\ProcurementSyncRun;
use App\Models\SpseSession;

class SpseSyncService
{
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

        foreach (self::SOURCES as $jenis => $source) {
            try {
                $result = $this->fetchAllPages(
                    $session,
                    $source['endpoint'],
                    $pageLength,
                    $source['referer'],
                );
                foreach ($result as $row) {
                    $staging = $this->storeRow($run, $jenis, $row);
                    $staging = $this->matchingService->matchStaging($staging);
                    $totalItems++;
                    if ($staging->match_status !== 'unmatched') {
                        $matchedCount++;
                    }
                }
            } catch (\Throwable $e) {
                $errors[] = $jenis.': '.$e->getMessage();
            }
        }

        $run->update([
            'status' => $errors === [] ? 'completed' : ($totalItems > 0 ? 'partial' : 'failed'),
            'item_count' => $totalItems,
            'matched_count' => $matchedCount,
            'error_log' => $errors === [] ? null : implode("\n", $errors),
            'finished_at' => now(),
        ]);

        return $run->fresh(['stagingPakets.pekerjaan', 'stagingPakets.kontrak']);
    }

    /**
     * @return array<int, array<int, mixed>>
     */
    private function fetchAllPages(
        SpseSession $session,
        string $endpoint,
        int $pageLength,
        string $refererPath,
    ): array {
        $allRows = [];
        $start = 0;
        $draw = 1;
        $pageLength = max(1, min($pageLength, 500));

        do {
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
                break;
            }

            foreach ($rows as $row) {
                if (is_array($row)) {
                    $allRows[] = $row;
                }
            }

            if (count($rows) < $pageLength) {
                break;
            }

            $start += $pageLength;
            $draw++;
        } while ($draw <= 50);

        return $allRows;
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
            'kode_paket' => (string) ($row[0] ?? ''),
            'nama_paket' => (string) ($row[1] ?? ''),
            'status_paket' => isset($row[2]) ? (string) $row[2] : null,
            'metode_pengadaan' => isset($row[5]) ? (string) $row[5] : null,
            'raw_row' => $row,
            'fetched_at' => now(),
        ]);
    }
}