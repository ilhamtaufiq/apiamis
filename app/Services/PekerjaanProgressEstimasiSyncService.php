<?php

namespace App\Services;

use App\Models\Kontrak;
use App\Models\PekerjaanProgressEstimasiHistory;
use App\Models\PuspenProgressFisik;
use Illuminate\Support\Collection;

class PekerjaanProgressEstimasiSyncService
{
    public function __construct(
        private readonly PekerjaanProgressEstimasiSummaryService $summaryService,
    ) {
    }

    public function syncToPuspenFromPekerjaan(int $pekerjaanId, int $tahunAnggaran): void
    {
        $histories = PekerjaanProgressEstimasiHistory::query()
            ->where('pekerjaan_id', $pekerjaanId)
            ->where('tahun_anggaran', $tahunAnggaran)
            ->get();

        $summary = $this->summaryService->summarize($histories, $tahunAnggaran);
        $rencana = $summary['fisik_rencana'];
        $realisasi = $summary['fisik_realisasi'];

        foreach ($this->resolveKontrakIds($pekerjaanId) as $kontrakId) {
            $existing = PuspenProgressFisik::query()
                ->where('kontrak_id', $kontrakId)
                ->where('tahun_anggaran', $tahunAnggaran)
                ->first();

            if (
                $existing
                && $this->nullableFloatEquals($existing->rencana, $rencana)
                && $this->nullableFloatEquals($existing->realisasi, $realisasi)
            ) {
                continue;
            }

            PuspenProgressFisik::updateOrCreate(
                [
                    'kontrak_id' => $kontrakId,
                    'tahun_anggaran' => $tahunAnggaran,
                ],
                [
                    'rencana' => $rencana,
                    'realisasi' => $realisasi,
                ],
            );
        }
    }
    public function syncFromPuspenKontrak(
        int $kontrakId,
        int $tahunAnggaran,
        ?float $rencana,
        ?float $realisasi,
        ?string $tanggal = null,
    ): void {
        $tanggal = $tanggal ?? now()->toDateString();
        $pekerjaanIds = $this->resolvePekerjaanIds($kontrakId);

        foreach ($pekerjaanIds as $pekerjaanId) {
            if ($rencana !== null) {
                $this->upsertHistoryEntry($pekerjaanId, $tahunAnggaran, 'fisik', 'rencana', $tanggal, $rencana);
            }

            if ($realisasi !== null) {
                $this->upsertHistoryEntry($pekerjaanId, $tahunAnggaran, 'fisik', 'realisasi', $tanggal, $realisasi);
            }
        }
    }

    /**
     * @return Collection<int, int>
     */
    private function resolvePekerjaanIds(int $kontrakId): Collection
    {
        $kontrak = Kontrak::query()
            ->with('pekerjaans:id')
            ->find($kontrakId);

        if (! $kontrak) {
            return collect();
        }

        return collect([$kontrak->id_pekerjaan])
            ->merge($kontrak->pekerjaans->pluck('id'))
            ->filter()
            ->unique()
            ->values();
    }

    private function upsertHistoryEntry(
        int $pekerjaanId,
        int $tahunAnggaran,
        string $jenis,
        string $tipe,
        string $tanggal,
        float $persen,
    ): void {
        $persen = round($persen, 2);

        $latest = PekerjaanProgressEstimasiHistory::query()
            ->where('pekerjaan_id', $pekerjaanId)
            ->where('tahun_anggaran', $tahunAnggaran)
            ->where('jenis', $jenis)
            ->where('tipe', $tipe)
            ->orderByDesc('tanggal')
            ->orderByDesc('id')
            ->first();

        if ($latest && (float) $latest->persen === $persen) {
            return;
        }

        $sameDay = PekerjaanProgressEstimasiHistory::query()
            ->where('pekerjaan_id', $pekerjaanId)
            ->where('tahun_anggaran', $tahunAnggaran)
            ->where('jenis', $jenis)
            ->where('tipe', $tipe)
            ->whereDate('tanggal', $tanggal)
            ->first();

        if ($sameDay) {
            $sameDay->update(['persen' => $persen]);

            return;
        }

        PekerjaanProgressEstimasiHistory::create([
            'pekerjaan_id' => $pekerjaanId,
            'tahun_anggaran' => $tahunAnggaran,
            'jenis' => $jenis,
            'tipe' => $tipe,
            'tanggal' => $tanggal,
            'persen' => $persen,
        ]);
    }

    /**
     * @return Collection<int, int>
     */
    private function resolveKontrakIds(int $pekerjaanId): Collection
    {
        return Kontrak::query()
            ->where('id_pekerjaan', $pekerjaanId)
            ->orWhereHas('pekerjaans', fn ($q) => $q->where('pekerjaan_id', $pekerjaanId))
            ->pluck('id')
            ->unique()
            ->values();
    }

    private function nullableFloatEquals(mixed $left, mixed $right): bool
    {
        if ($left === null && $right === null) {
            return true;
        }

        if ($left === null || $right === null) {
            return false;
        }

        return round((float) $left, 2) === round((float) $right, 2);
    }
}