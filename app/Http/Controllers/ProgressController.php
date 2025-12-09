<?php

namespace App\Http\Controllers;

use App\Models\Progress;
use App\Models\Pekerjaan;
use Illuminate\Http\Request;
use Illuminate\Http\JsonResponse;

class ProgressController extends Controller
{
    /**
     * Get progress report for a pekerjaan
     */
    public function report(int $pekerjaanId): JsonResponse
    {
        $pekerjaan = Pekerjaan::with(['kegiatan', 'kontrak.penyedia', 'kecamatan', 'desa'])->findOrFail($pekerjaanId);
        
        // Get first kontrak (assuming one kontrak per pekerjaan)
        $kontrak = $pekerjaan->kontrak->first();
        $penyedia = $kontrak?->penyedia;
        $kegiatan = $pekerjaan->kegiatan;
        
        $progress = Progress::firstOrCreate(
            ['pekerjaan_id' => $pekerjaanId],
            ['content' => ['items' => [], 'week_count' => 4]]
        );

        $content = $progress->content ?? ['items' => [], 'week_count' => 4];
        $items = $content['items'] ?? [];

        // Calculate totals on the fly for display
        $totalBobot = 0;
        $totalAccumulatedReal = 0;
        $totalWeightedProgress = 0;
        $maxMinggu = 0;

        foreach ($items as $item) {
            $bobot = (float) ($item['bobot'] ?? 0);
            $totalBobot += $bobot;

            $weeklyData = $item['weekly_data'] ?? [];
            $itemTotalReal = 0;
            $itemMaxMinggu = 0;

            foreach ($weeklyData as $minggu => $data) {
                $realisasi = $data['realisasi'] ?? 0;
                if ($realisasi !== null) {
                    $itemTotalReal += $realisasi;
                }
                $itemMaxMinggu = max($itemMaxMinggu, (int)$minggu);
            }

            $totalAccumulatedReal += $itemTotalReal;
            $maxMinggu = max($maxMinggu, $itemMaxMinggu);
            
            $targetVolume = (float) ($item['target_volume'] ?? 0);
            $progressPercent = $targetVolume > 0 
                ? ($itemTotalReal / $targetVolume) * 100 
                : 0;
            $weightedProgress = ($progressPercent * $bobot) / 100;
            $totalWeightedProgress += $weightedProgress;
        }

        return response()->json([
            'success' => true,
            'data' => [
                'pekerjaan' => [
                    'id' => $pekerjaan->id,
                    'nama' => $pekerjaan->nama_paket,
                    'pagu' => $pekerjaan->pagu,
                    'lokasi' => ($pekerjaan->desa?->nama ?? '') . ', ' . ($pekerjaan->kecamatan?->nama ?? ''),
                ],
                'kegiatan' => $kegiatan ? [
                    'nama_kegiatan' => $kegiatan->nama_kegiatan,
                    'nama_sub_kegiatan' => $kegiatan->nama_sub_kegiatan,
                    'sumber_dana' => $kegiatan->sumber_dana,
                    'tahun_anggaran' => $kegiatan->tahun_anggaran,
                ] : null,
                'kontrak' => $kontrak ? [
                    'tgl_spmk' => $kontrak->tgl_spmk?->format('Y-m-d'),
                    'tgl_spk' => $kontrak->tgl_spk?->format('Y-m-d'),
                    'tgl_selesai' => $kontrak->tgl_selesai?->format('Y-m-d'),
                    'spk' => $kontrak->spk,
                    'spmk' => $kontrak->spmk,
                    'nilai_kontrak' => $kontrak->nilai_kontrak,
                ] : null,
                'penyedia' => $penyedia ? [
                    'nama' => $penyedia->nama,
                    'direktur' => $penyedia->direktur,
                ] : null,
                'items' => $items,
                'totals' => [
                    'total_bobot' => $totalBobot,
                    'total_accumulated_real' => $totalAccumulatedReal,
                    'total_weighted_progress' => $totalWeightedProgress,
                ],
                'max_minggu' => max($maxMinggu, $content['week_count'] ?? 4),
            ],
        ]);
    }

    /**
     * Store full progress report
     */
    public function store(Request $request, int $pekerjaanId): JsonResponse
    {
        $validated = $request->validate([
            'items' => 'present|array',
            'items.*.nama_item' => 'required|string',
            'items.*.rincian_item' => 'nullable|string',
            'items.*.satuan' => 'required|string',
            'items.*.harga_satuan' => 'nullable|numeric',
            'items.*.bobot' => 'nullable|numeric',
            'items.*.target_volume' => 'nullable|numeric',
            'items.*.weekly_data' => 'nullable|array',
            'week_count' => 'required|integer|min:1',
        ]);

        $progress = Progress::updateOrCreate(
            ['pekerjaan_id' => $pekerjaanId],
            ['content' => $validated]
        );

        return response()->json([
            'success' => true,
            'message' => 'Progress berhasil disimpan',
            'data' => $progress->content,
        ]);
    }
}
