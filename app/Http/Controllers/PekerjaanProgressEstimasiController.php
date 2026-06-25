<?php

namespace App\Http\Controllers;

use App\Http\Resources\PekerjaanProgressEstimasiResource;
use App\Models\Kontrak;
use App\Models\Pekerjaan;
use App\Models\PekerjaanProgressEstimasiHistory;
use App\Models\PuspenProgressFisik;
use App\Services\PekerjaanProgressEstimasiSyncService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class PekerjaanProgressEstimasiController extends Controller
{
    public function show(Request $request, int $pekerjaanId): JsonResponse
    {
        $validated = $request->validate([
            'tahun' => 'nullable|integer|min:2000|max:2100',
        ]);

        $tahun = (int) ($validated['tahun'] ?? now()->year);
        $pekerjaan = Pekerjaan::byUserRole()->findOrFail($pekerjaanId);

        return response()->json([
            'data' => new PekerjaanProgressEstimasiResource($this->buildPayload($pekerjaan->id, $tahun)),
            'puspen_progress_fisik' => $this->getPuspenSnapshot($pekerjaan->id, $tahun),
        ]);
    }

    public function update(
        Request $request,
        int $pekerjaanId,
        PekerjaanProgressEstimasiSyncService $syncService,
    ): JsonResponse
    {
        $validated = $request->validate([
            'tahun' => 'required|integer|min:2000|max:2100',
            'fisik' => 'nullable|array',
            'fisik.rencana' => 'nullable|array',
            'fisik.rencana.*.tanggal' => 'required|date',
            'fisik.rencana.*.persen' => ['required', fn ($attr, $value, $fail) => $this->validatePercent($attr, $value, $fail)],
            'fisik.realisasi' => 'nullable|array',
            'fisik.realisasi.*.tanggal' => 'required|date',
            'fisik.realisasi.*.persen' => ['required', fn ($attr, $value, $fail) => $this->validatePercent($attr, $value, $fail)],
            'keuangan' => 'nullable|array',
            'keuangan.rencana' => 'nullable|array',
            'keuangan.rencana.*.tanggal' => 'required|date',
            'keuangan.rencana.*.persen' => ['required', fn ($attr, $value, $fail) => $this->validatePercent($attr, $value, $fail)],
            'keuangan.realisasi' => 'nullable|array',
            'keuangan.realisasi.*.tanggal' => 'required|date',
            'keuangan.realisasi.*.persen' => ['required', fn ($attr, $value, $fail) => $this->validatePercent($attr, $value, $fail)],
        ]);

        $pekerjaan = Pekerjaan::byUserRole()->findOrFail($pekerjaanId);
        $tahun = (int) $validated['tahun'];

        DB::transaction(function () use ($pekerjaan, $tahun, $validated) {
            PekerjaanProgressEstimasiHistory::query()
                ->where('pekerjaan_id', $pekerjaan->id)
                ->where('tahun_anggaran', $tahun)
                ->delete();

            foreach (['fisik', 'keuangan'] as $jenis) {
                foreach (['rencana', 'realisasi'] as $tipe) {
                    $entries = data_get($validated, "{$jenis}.{$tipe}", []);

                    foreach ($entries as $entry) {
                        PekerjaanProgressEstimasiHistory::create([
                            'pekerjaan_id' => $pekerjaan->id,
                            'tahun_anggaran' => $tahun,
                            'jenis' => $jenis,
                            'tipe' => $tipe,
                            'tanggal' => $entry['tanggal'],
                            'persen' => $this->normalizePercent($entry['persen']),
                        ]);
                    }
                }
            }
        });

        $syncService->syncToPuspenFromPekerjaan($pekerjaan->id, $tahun);

        return response()->json([
            'message' => 'Riwayat progress estimasi berhasil disimpan dan disinkronkan ke Puspen progress fisik',
            'data' => new PekerjaanProgressEstimasiResource($this->buildPayload($pekerjaan->id, $tahun)),
            'puspen_progress_fisik' => $this->getPuspenSnapshot($pekerjaan->id, $tahun),
        ]);
    }

    private function buildPayload(int $pekerjaanId, int $tahun): array
    {
        $histories = PekerjaanProgressEstimasiHistory::query()
            ->where('pekerjaan_id', $pekerjaanId)
            ->where('tahun_anggaran', $tahun)
            ->orderBy('tanggal')
            ->orderBy('id')
            ->get();

        $grouped = [
            'fisik' => ['rencana' => [], 'realisasi' => []],
            'keuangan' => ['rencana' => [], 'realisasi' => []],
        ];

        foreach ($histories as $history) {
            $grouped[$history->jenis][$history->tipe][] = [
                'id' => $history->id,
                'tanggal' => $history->tanggal->format('Y-m-d'),
                'persen' => $history->persen,
            ];
        }

        $latestUpdatedAt = $histories->max('updated_at');

        return [
            'pekerjaan_id' => $pekerjaanId,
            'tahun_anggaran' => $tahun,
            'fisik' => $this->buildSectionSummary($grouped['fisik']),
            'keuangan' => $this->buildSectionSummary($grouped['keuangan']),
            'updated_at' => $latestUpdatedAt?->toISOString(),
        ];
    }

    private function buildSectionSummary(array $section): array
    {
        $latestRencana = $this->latestEntry($section['rencana']);
        $latestRealisasi = $this->latestEntry($section['realisasi']);

        return [
            'rencana' => $section['rencana'],
            'realisasi' => $section['realisasi'],
            'latest_rencana' => $latestRencana['persen'] ?? null,
            'latest_realisasi' => $latestRealisasi['persen'] ?? null,
            'deviasi' => $latestRencana !== null && $latestRealisasi !== null
                ? round(($latestRealisasi['persen'] ?? 0) - ($latestRencana['persen'] ?? 0), 2)
                : null,
        ];
    }

    private function latestEntry(array $entries): ?array
    {
        if ($entries === []) {
            return null;
        }

        return collect($entries)
            ->sortBy([
                ['tanggal', 'desc'],
                ['id', 'desc'],
            ])
            ->first();
    }

    private function getPuspenSnapshot(int $pekerjaanId, int $tahun): array
    {
        $kontrakIds = Kontrak::query()
            ->where('id_pekerjaan', $pekerjaanId)
            ->orWhereHas('pekerjaans', fn ($q) => $q->where('pekerjaan_id', $pekerjaanId))
            ->pluck('id');

        if ($kontrakIds->isEmpty()) {
            return [];
        }

        return PuspenProgressFisik::query()
            ->whereIn('kontrak_id', $kontrakIds)
            ->where('tahun_anggaran', $tahun)
            ->with('kontrak:id,kode_paket')
            ->get()
            ->map(function (PuspenProgressFisik $item) {
                $rencana = $item->rencana;
                $realisasi = $item->realisasi;

                return [
                    'kontrak_id' => $item->kontrak_id,
                    'kode_paket' => $item->kontrak?->kode_paket,
                    'rencana' => $rencana,
                    'realisasi' => $realisasi,
                    'deviasi' => $rencana !== null && $realisasi !== null
                        ? round($realisasi - $rencana, 2)
                        : null,
                    'updated_at' => $item->updated_at?->toISOString(),
                ];
            })
            ->values()
            ->all();
    }

    private function validatePercent(string $attribute, mixed $value, \Closure $fail): void
    {
        if ($value === null || $value === '') {
            $fail("Field {$attribute} wajib diisi.");

            return;
        }

        $stringValue = trim((string) $value);

        if (preg_match('/^\d+([.,]\d{1,2})?$/', $stringValue) !== 1) {
            $fail("Field {$attribute} harus berupa angka desimal dengan maksimal 2 angka di belakang koma.");

            return;
        }

        $normalized = str_replace(',', '.', $stringValue);
        if (! is_numeric($normalized)) {
            $fail("Field {$attribute} harus berupa angka desimal.");

            return;
        }

        $numeric = (float) $normalized;
        if ($numeric < 0 || $numeric > 100) {
            $fail("Field {$attribute} harus berada antara 0 dan 100.");
        }
    }

    private function normalizePercent(mixed $value): float
    {
        $normalized = str_replace(',', '.', trim((string) $value));

        return round((float) $normalized, 2);
    }
}