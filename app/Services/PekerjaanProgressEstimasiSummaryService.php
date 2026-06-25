<?php

namespace App\Services;

use Illuminate\Support\Collection;

class PekerjaanProgressEstimasiSummaryService
{
    /**
     * @param  Collection<int, \App\Models\PekerjaanProgressEstimasiHistory>  $histories
     * @return array{
     *     fisik_realisasi: float|null,
     *     fisik_rencana: float|null,
     *     fisik_deviasi: float|null,
     *     keuangan_realisasi: float|null,
     *     keuangan_rencana: float|null,
     *     keuangan_deviasi: float|null
     * }
     */
    public function summarize(Collection $histories, int $tahunAnggaran): array
    {
        $filtered = $histories->where('tahun_anggaran', $tahunAnggaran);

        $grouped = [
            'fisik' => ['rencana' => [], 'realisasi' => []],
            'keuangan' => ['rencana' => [], 'realisasi' => []],
        ];

        foreach ($filtered as $history) {
            $grouped[$history->jenis][$history->tipe][] = [
                'id' => $history->id,
                'tanggal' => $history->tanggal->format('Y-m-d'),
                'persen' => (float) $history->persen,
            ];
        }

        $fisik = $this->buildSectionSummary($grouped['fisik']);
        $keuangan = $this->buildSectionSummary($grouped['keuangan']);

        return [
            'fisik_realisasi' => $fisik['latest_realisasi'],
            'fisik_rencana' => $fisik['latest_rencana'],
            'fisik_deviasi' => $fisik['deviasi'],
            'keuangan_realisasi' => $keuangan['latest_realisasi'],
            'keuangan_rencana' => $keuangan['latest_rencana'],
            'keuangan_deviasi' => $keuangan['deviasi'],
        ];
    }

    private function buildSectionSummary(array $section): array
    {
        $latestRencana = $this->latestEntry($section['rencana']);
        $latestRealisasi = $this->latestEntry($section['realisasi']);

        return [
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
}