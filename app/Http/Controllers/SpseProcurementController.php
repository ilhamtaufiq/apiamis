<?php

namespace App\Http\Controllers;

use App\Models\Kontrak;
use App\Models\ProcurementStagingPaket;
use App\Models\ProcurementSyncRun;
use App\Services\Procurement\ProcurementMatchingService;
use App\Services\Procurement\SpseBerkasImportService;
use App\Services\Procurement\SpseDocumentScanner;
use App\Services\Procurement\SpseDocumentZipService;
use App\Services\Procurement\SpseSessionStore;
use App\Services\Procurement\SpseKontrakPushService;
use App\Services\Procurement\SpseSyncService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

class SpseProcurementController extends Controller
{
    public function sessionStatus(SpseSessionStore $sessionStore): JsonResponse
    {
        return response()->json($sessionStore->status(auth()->id()));
    }

    public function saveSession(Request $request, SpseSessionStore $sessionStore): JsonResponse
    {
        $validated = $request->validate([
            'cookie_header' => 'nullable|string|max:20000',
            'cookies' => 'nullable|array',
            'cookies.*.name' => 'required_with:cookies|string|max:255',
            'cookies.*.value' => 'required_with:cookies|string|max:5000',
            'cookies.*.domain' => 'nullable|string|max:255',
            'cookies.*.path' => 'nullable|string|max:255',
            'lpse_slug' => 'nullable|string|max:64',
        ]);

        if (empty($validated['cookie_header']) && empty($validated['cookies'])) {
            return response()->json(['message' => 'cookie_header atau cookies wajib diisi.'], 422);
        }

        try {
            $session = $sessionStore->save(
                auth()->id(),
                $validated['cookie_header'] ?? null,
                $validated['cookies'] ?? null,
                $validated['lpse_slug'] ?? null,
            );

            return response()->json([
                'message' => 'Session SPSE tersimpan.',
                'session' => [
                    'id' => $session->id,
                    'lpse_slug' => $session->lpse_slug,
                    'expires_at' => $session->expires_at?->toIso8601String(),
                ],
            ]);
        } catch (\InvalidArgumentException $e) {
            return response()->json(['message' => $e->getMessage()], 422);
        } catch (\RuntimeException $e) {
            return response()->json(['message' => $e->getMessage()], 400);
        }
    }

    public function revokeSession(SpseSessionStore $sessionStore): JsonResponse
    {
        $sessionStore->revoke(auth()->id());

        return response()->json(['message' => 'Session SPSE dihapus.']);
    }

    public function sync(Request $request, SpseSessionStore $sessionStore, SpseSyncService $syncService): JsonResponse
    {
        $session = $sessionStore->activeSession(auth()->id());
        if (! $session) {
            return response()->json(['message' => 'Session SPSE tidak aktif. Login ulang di SPSE.'], 401);
        }

        $validated = $request->validate([
            'page_length' => 'nullable|integer|min:1|max:500',
        ]);

        try {
            $run = $syncService->sync(
                $session,
                auth()->id(),
                (int) ($validated['page_length'] ?? 100),
            );

            return response()->json([
                'message' => 'Sync SPSE selesai.',
                'run' => $this->formatRun($run),
            ]);
        } catch (\Throwable $e) {
            return response()->json(['message' => 'Sync SPSE gagal: '.$e->getMessage()], 500);
        }
    }

    public function syncRuns(Request $request): JsonResponse
    {
        $runs = ProcurementSyncRun::query()
            ->where('user_id', auth()->id())
            ->orderByDesc('id')
            ->limit(20)
            ->get()
            ->map(fn (ProcurementSyncRun $run) => $this->formatRun($run));

        return response()->json(['data' => $runs]);
    }

    public function staging(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'sync_run_id' => 'nullable|integer|exists:tbl_procurement_sync_runs,id',
            'match_status' => 'nullable|string|in:unmatched,exact_kode_paket,fuzzy_nama_paket,manual_map',
            'search' => 'nullable|string|max:200',
            'tahun' => 'nullable|string|max:4',
            'page' => 'nullable|integer|min:1',
            'per_page' => 'nullable|integer|min:1|max:200',
        ]);

        $query = ProcurementStagingPaket::query()
            ->with(['pekerjaan:id,nama_paket', 'kontrak:id,kode_paket,spk'])
            ->orderByDesc('id');

        if (! empty($validated['sync_run_id'])) {
            $query->where('sync_run_id', $validated['sync_run_id']);
        } else {
            $latestRunId = ProcurementSyncRun::query()
                ->where('user_id', auth()->id())
                ->orderByDesc('id')
                ->value('id');
            if ($latestRunId) {
                $query->where('sync_run_id', $latestRunId);
            }
        }

        if (! empty($validated['match_status'])) {
            $query->where('match_status', $validated['match_status']);
        }

        if (! empty($validated['search'])) {
            $search = $validated['search'];
            $query->where(function ($q) use ($search) {
                $q->where('nama_paket', 'like', "%{$search}%")
                    ->orWhere('kode_paket', 'like', "%{$search}%");
            });
        }

        if (! empty($validated['tahun'])) {
            $tahun = $validated['tahun'];
            $query->where(function ($q) use ($tahun) {
                $q->where('match_status', 'unmatched')
                    ->orWhereHas('pekerjaan.kegiatan', function ($kegiatanQuery) use ($tahun) {
                        $kegiatanQuery->where('tahun_anggaran', $tahun);
                    });
            });
        }

        $perPage = (int) ($validated['per_page'] ?? 20);

        return response()->json($query->paginate($perPage));
    }

    public function stagingDetail(int $id): JsonResponse
    {
        $staging = ProcurementStagingPaket::query()
            ->with([
                'pekerjaan:id,nama_paket,kegiatan_id,kecamatan_id,desa_id,pagu,kode_rekening',
                'pekerjaan.kegiatan:id,nama_kegiatan,tahun_anggaran',
                'pekerjaan.kecamatan:id,nama',
                'pekerjaan.desa:id,nama',
                'kontrak:id,kode_paket,spk,nilai_kontrak,tgl_spk',
                'syncRun:id,status,started_at,finished_at,item_count,matched_count',
            ])
            ->whereHas('syncRun', function ($query) {
                $query->where('user_id', auth()->id());
            })
            ->findOrFail($id);

        $lpseSlug = config('services.spse.lpse_slug', 'cianjurkab');
        $baseUrl = rtrim(config('services.spse.base_url', 'https://spse.inaproc.id'), '/');
        $path = $staging->jenis_paket === 'tender_seleksi'
            ? "/tender/{$staging->kode_paket}"
            : "/nontender/{$staging->kode_paket}";

        return response()->json([
            'data' => [
                'id' => $staging->id,
                'sync_run_id' => $staging->sync_run_id,
                'sumber' => $staging->sumber,
                'kode_paket' => $staging->kode_paket,
                'nama_paket' => $staging->nama_paket,
                'status_paket' => $staging->status_paket,
                'metode_pengadaan' => $staging->metode_pengadaan,
                'jenis_paket' => $staging->jenis_paket,
                'matched_pekerjaan_id' => $staging->matched_pekerjaan_id,
                'matched_kontrak_id' => $staging->matched_kontrak_id,
                'match_status' => $staging->match_status,
                'raw_row' => $staging->raw_row,
                'fetched_at' => $staging->fetched_at?->toIso8601String(),
                'spse_url' => "{$baseUrl}/{$lpseSlug}{$path}",
                'pekerjaan' => $staging->pekerjaan,
                'kontrak' => $staging->kontrak,
                'sync_run' => $staging->syncRun ? $this->formatRun($staging->syncRun) : null,
            ],
        ]);
    }

    public function applyStaging(Request $request, ProcurementMatchingService $matchingService): JsonResponse
    {
        $validated = $request->validate([
            'ids' => 'required|array|min:1',
            'ids.*' => 'integer|exists:tbl_procurement_staging_paket,id',
            'overwrite' => 'nullable|boolean',
        ]);

        $applied = 0;
        $skipped = 0;
        $results = [];

        $items = ProcurementStagingPaket::query()->whereIn('id', $validated['ids'])->get();
        foreach ($items as $item) {
            if ($item->match_status === 'unmatched' && ! $item->matched_pekerjaan_id) {
                $skipped++;
                $results[] = ['id' => $item->id, 'status' => 'skipped', 'reason' => 'unmatched'];

                continue;
            }

            $kontrak = $matchingService->applyToKontrak($item, (bool) ($validated['overwrite'] ?? false));
            if ($kontrak) {
                $applied++;
                $results[] = [
                    'id' => $item->id,
                    'status' => 'applied',
                    'kontrak_id' => $kontrak->id,
                    'kode_paket' => $kontrak->kode_paket,
                ];
            } else {
                $skipped++;
                $results[] = ['id' => $item->id, 'status' => 'skipped', 'reason' => 'no_kontrak'];
            }
        }

        return response()->json([
            'message' => "Apply selesai: {$applied} berhasil, {$skipped} dilewati.",
            'applied' => $applied,
            'skipped' => $skipped,
            'results' => $results,
        ]);
    }

    public function packageDocuments(
        Request $request,
        string $kodePaket,
        SpseSessionStore $sessionStore,
        SpseDocumentScanner $scanner,
    ): JsonResponse {
        $session = $sessionStore->activeSession(auth()->id());
        if (! $session) {
            return response()->json(['message' => 'Session SPSE tidak aktif. Login ulang di SPSE.'], 401);
        }

        $validated = $request->validate([
            'jenis_paket' => 'nullable|string|in:pengadaan_langsung,tender_seleksi',
        ]);

        try {
            $documents = $scanner->scan(
                $session,
                $kodePaket,
                $validated['jenis_paket'] ?? null,
            );

            return response()->json([
                'kode_paket' => $kodePaket,
                'count' => count($documents),
                'data' => $documents,
            ]);
        } catch (\InvalidArgumentException $e) {
            return response()->json(['message' => $e->getMessage()], 422);
        } catch (\Throwable $e) {
            return response()->json(['message' => 'Gagal memindai dokumen SPSE: '.$e->getMessage()], 500);
        }
    }

    public function importPackageDocuments(
        Request $request,
        SpseSessionStore $sessionStore,
        SpseBerkasImportService $importService,
    ): JsonResponse {
        $session = $sessionStore->activeSession(auth()->id());
        if (! $session) {
            return response()->json(['message' => 'Session SPSE tidak aktif. Login ulang di SPSE.'], 401);
        }

        $validated = $request->validate([
            'pekerjaan_id' => 'required|integer|exists:tbl_pekerjaan,id',
            'kode_paket' => 'required|string|max:64',
            'documents' => 'required|array|min:1|max:50',
            'documents.*.url' => 'required|string|max:2000',
            'documents.*.jenis_dokumen' => 'required|string|max:255',
            'documents.*.label' => 'nullable|string|max:255',
        ]);

        try {
            $result = $importService->import(
                $session,
                (int) $validated['pekerjaan_id'],
                $validated['documents'],
            );

            return response()->json([
                'message' => "Import selesai: {$result['imported']} berhasil, {$result['failed']} gagal.",
                'kode_paket' => $validated['kode_paket'],
                'pekerjaan_id' => (int) $validated['pekerjaan_id'],
                'imported' => $result['imported'],
                'failed' => $result['failed'],
                'results' => $result['results'],
            ]);
        } catch (\Throwable $e) {
            return response()->json(['message' => 'Import dokumen SPSE gagal: '.$e->getMessage()], 500);
        }
    }

    public function downloadPackageZip(
        Request $request,
        SpseSessionStore $sessionStore,
        SpseDocumentZipService $zipService,
    ) {
        $session = $sessionStore->activeSession(auth()->id());
        if (! $session) {
            return response()->json(['message' => 'Session SPSE tidak aktif. Login ulang di SPSE.'], 401);
        }

        $validated = $request->validate([
            'kode_paket' => 'required|string|max:64',
            'documents' => 'required|array|min:1|max:50',
            'documents.*.url' => 'required|string|max:2000',
            'documents.*.label' => 'nullable|string|max:255',
        ]);

        try {
            $result = $zipService->buildZip(
                $session,
                $validated['kode_paket'],
                $validated['documents'],
            );

            if ($result['count'] === 0) {
                return response()->json([
                    'message' => 'Tidak ada dokumen berhasil diunduh.',
                    'failed' => $result['failed'],
                    'failed_details' => $result['failed_details'],
                ], 422);
            }

            return response()
                ->download($result['path'], $result['filename'], ['Content-Type' => 'application/zip'])
                ->deleteFileAfterSend(true);
        } catch (\Throwable $e) {
            return response()->json(['message' => 'Unduh ZIP gagal: '.$e->getMessage()], 500);
        }
    }

    public function pushKontrak(
        Request $request,
        SpseSessionStore $sessionStore,
        SpseKontrakPushService $pushService,
    ): JsonResponse {
        $session = $sessionStore->activeSession(auth()->id());
        if (! $session) {
            return response()->json(['message' => 'Session SPSE tidak aktif. Login ulang di SPSE.'], 401);
        }

        $validated = $request->validate([
            'kontrak_id' => 'required|integer|exists:tbl_kontrak,id',
        ]);

        $kontrak = Kontrak::query()
            ->with(['penyedia', 'pekerjaans'])
            ->findOrFail($validated['kontrak_id']);

        try {
            $result = $pushService->push($kontrak, $session);

            return response()->json($result);
        } catch (\InvalidArgumentException $e) {
            return response()->json(['message' => $e->getMessage()], 422);
        } catch (\Throwable $e) {
            return response()->json(['message' => 'Push kontrak ke SPSE gagal: '.$e->getMessage()], 500);
        }
    }

    public function mapStaging(Request $request, ProcurementMatchingService $matchingService): JsonResponse
    {
        $validated = $request->validate([
            'id' => 'required|integer|exists:tbl_procurement_staging_paket,id',
            'pekerjaan_id' => 'required|integer|exists:tbl_pekerjaan,id',
        ]);

        $staging = ProcurementStagingPaket::query()->findOrFail($validated['id']);
        $pekerjaan = \App\Models\Pekerjaan::query()->findOrFail($validated['pekerjaan_id']);
        $kontrak = $pekerjaan->kontrak()->first();

        $staging->update([
            'matched_pekerjaan_id' => $pekerjaan->id,
            'matched_kontrak_id' => $kontrak?->id,
            'match_status' => 'manual_map',
        ]);

        return response()->json([
            'message' => 'Mapping manual tersimpan.',
            'staging' => $staging->fresh(['pekerjaan', 'kontrak']),
        ]);
    }

    /**
     * Promote unmatched staging rows into draft pekerjaan + kontrak shell.
     */
    public function promoteStaging(Request $request, ProcurementMatchingService $matchingService): JsonResponse
    {
        $validated = $request->validate([
            'ids' => 'required|array|min:1|max:50',
            'ids.*' => 'integer|exists:tbl_procurement_staging_paket,id',
            'kegiatan_id' => 'nullable|integer|exists:tbl_kegiatan,id',
            'is_konsultan' => 'nullable|boolean',
        ]);

        $created = 0;
        $skipped = 0;
        $results = [];

        $items = ProcurementStagingPaket::query()->whereIn('id', $validated['ids'])->get();
        foreach ($items as $item) {
            try {
                $result = $matchingService->promoteToDraft($item, [
                    'kegiatan_id' => $validated['kegiatan_id'] ?? null,
                    'is_konsultan' => $validated['is_konsultan'] ?? true,
                ]);

                if ($result['status'] === 'created') {
                    $created++;
                    $results[] = [
                        'id' => $item->id,
                        'status' => 'created',
                        'pekerjaan_id' => $result['pekerjaan']?->id,
                        'kontrak_id' => $result['kontrak']?->id,
                    ];
                } else {
                    $skipped++;
                    $results[] = [
                        'id' => $item->id,
                        'status' => 'skipped',
                        'reason' => $result['status'],
                        'pekerjaan_id' => $result['pekerjaan']?->id,
                    ];
                }
            } catch (\Throwable $e) {
                $skipped++;
                $results[] = [
                    'id' => $item->id,
                    'status' => 'error',
                    'reason' => $e->getMessage(),
                ];
            }
        }

        return response()->json([
            'message' => "Promote draft: {$created} dibuat, {$skipped} dilewati.",
            'created' => $created,
            'skipped' => $skipped,
            'results' => $results,
        ]);
    }

    private function formatRun(ProcurementSyncRun $run): array
    {
        return [
            'id' => $run->id,
            'status' => $run->status,
            'item_count' => $run->item_count,
            'matched_count' => $run->matched_count,
            'error_log' => $run->error_log,
            'started_at' => $run->started_at?->toIso8601String(),
            'finished_at' => $run->finished_at?->toIso8601String(),
        ];
    }
}