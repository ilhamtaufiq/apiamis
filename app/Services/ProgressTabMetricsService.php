<?php

namespace App\Services;

/**
 * Metrik progress fisik selaras tab Progress / Buat Laporan (frontend).
 *
 * Bobot dihitung ulang dari nilai RAB item (volume × harga × 1,11 PPN),
 * bukan hanya field bobot tersimpan di content — supaya export & list
 * sama dengan totalWeightedProgress di ProgressReportEditor.
 */
class ProgressTabMetricsService
{
    public const RAB_PPN_RATE = 0.11;

    /**
     * @param  array<string, mixed>|null  $content  Progress.content
     * @return array{
     *     progress_total: float,
     *     progress_rencana: float,
     *     deviasi: float,
     *     max_reported_week: int,
     * }
     */
    public function summarize(?array $content): array
    {
        $items = is_array($content) ? ($content['items'] ?? []) : [];
        if (! is_array($items) || $items === []) {
            return [
                'progress_total' => 0.0,
                'progress_rencana' => 0.0,
                'deviasi' => 0.0,
                'max_reported_week' => 0,
            ];
        }

        $maxReportedWeek = 0;
        foreach ($items as $item) {
            if (! is_array($item)) {
                continue;
            }
            $weeklyData = $item['weekly_data'] ?? [];
            if (! is_array($weeklyData)) {
                continue;
            }
            foreach ($weeklyData as $minggu => $data) {
                if (! is_array($data)) {
                    continue;
                }
                if (array_key_exists('realisasi', $data) && $data['realisasi'] !== null && $data['realisasi'] !== '') {
                    $maxReportedWeek = max($maxReportedWeek, (int) $minggu);
                }
            }
        }

        // Total RAB base (dengan PPN) untuk bobot proporsional — mirror FE calculateProgressData
        $totalRabBase = 0.0;
        foreach ($items as $item) {
            if (! is_array($item)) {
                continue;
            }
            $totalRabBase += $this->itemRabValue(
                (float) ($item['target_volume'] ?? 0),
                (float) ($item['harga_satuan'] ?? 0),
            );
        }

        $progressTotal = 0.0;
        $progressRencana = 0.0;

        foreach ($items as $item) {
            if (! is_array($item)) {
                continue;
            }

            $targetVolume = (float) ($item['target_volume'] ?? 0);
            $itemRab = $this->itemRabValue($targetVolume, (float) ($item['harga_satuan'] ?? 0));

            // Prefer RAB-weighted bobot; fallback ke bobot tersimpan bila tidak ada harga
            if ($totalRabBase > 0 && $itemRab > 0) {
                $bobot = ($itemRab / $totalRabBase) * 100;
            } else {
                $bobot = (float) ($item['bobot'] ?? 0);
            }

            $weeklyData = is_array($item['weekly_data'] ?? null) ? $item['weekly_data'] : [];
            $itemTotalReal = 0.0;
            $itemTotalRencana = 0.0;

            foreach ($weeklyData as $minggu => $data) {
                if (! is_array($data)) {
                    continue;
                }

                $realisasi = $data['realisasi'] ?? null;
                if ($realisasi !== null && $realisasi !== '') {
                    $itemTotalReal += (float) $realisasi;
                }

                if ($maxReportedWeek > 0 && (int) $minggu <= $maxReportedWeek) {
                    $rencana = $data['rencana'] ?? null;
                    if ($rencana !== null && $rencana !== '') {
                        $itemTotalRencana += (float) $rencana;
                    }
                }
            }

            $progressPercent = $targetVolume > 0
                ? ($itemTotalReal / $targetVolume) * 100
                : 0.0;
            $progressTotal += ($progressPercent * $bobot) / 100;

            $rencanaPercent = $targetVolume > 0
                ? ($itemTotalRencana / $targetVolume) * 100
                : 0.0;
            $progressRencana += ($rencanaPercent * $bobot) / 100;
        }

        $progressTotal = round($progressTotal, 2);
        $progressRencana = round($progressRencana, 2);

        return [
            'progress_total' => $progressTotal,
            'progress_rencana' => $progressRencana,
            'deviasi' => round($progressTotal - $progressRencana, 2),
            'max_reported_week' => $maxReportedWeek,
        ];
    }

    public function itemRabValue(float $volume, float $hargaSatuan): float
    {
        return $volume * $hargaSatuan * (1 + self::RAB_PPN_RATE);
    }
}
