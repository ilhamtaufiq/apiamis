<?php

namespace App\Http\Controllers;

use App\Models\Progress;
use App\Models\Pekerjaan;
use Illuminate\Http\Request;
use Illuminate\Http\JsonResponse;

class ProgressController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/progress/{pekerjaanId}",
     *     summary="Get progress report for a pekerjaan",
     *     tags={"Progress"},
     *     @OA\Parameter(name="pekerjaanId", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function report(int $pekerjaanId): JsonResponse
    {
        $pekerjaan = Pekerjaan::with([
            'kegiatan',
            'kontrak.penyedia',
            'kontrak.latestApprovedAddendum',
            'kecamatan',
            'desa',
            'pengawas',
        ])->findOrFail($pekerjaanId);

        // Pivot kontrak_pekerjaan dulu; fallback id_pekerjaan di tbl_kontrak
        // (banyak paket hanya terhubung via id_pekerjaan tanpa baris pivot).
        $kontrak = $pekerjaan->kontrak->first();
        if (! $kontrak) {
            $kontrak = \App\Models\Kontrak::query()
                ->with(['penyedia', 'latestApprovedAddendum'])
                ->where('id_pekerjaan', $pekerjaanId)
                ->orderByDesc('id')
                ->first();
        }

        $penyedia = $kontrak?->penyedia;
        $kegiatan = $pekerjaan->kegiatan;
        $pengawas = $pekerjaan->pengawas;
        
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
                    'lokasi' => ($pekerjaan->desa?->n_desa ?? '') . ', ' . ($pekerjaan->kecamatan?->n_kec ?? ''),
                    'desa_nama' => $pekerjaan->desa?->n_desa,
                    'kecamatan_nama' => $pekerjaan->kecamatan?->n_kec,
                ],
                'kegiatan' => $kegiatan ? [
                    'nama_kegiatan' => $kegiatan->nama_kegiatan,
                    'nama_sub_kegiatan' => $kegiatan->nama_sub_kegiatan,
                    'sumber_dana' => $kegiatan->sumber_dana,
                    'tahun_anggaran' => $kegiatan->tahun_anggaran,
                    // PPTK di level sub kegiatan (autofill pejabat Mengetahui)
                    'nama_pptk' => $kegiatan->nama_pptk,
                    'nip_pptk' => $kegiatan->nip_pptk,
                ] : null,
                'kontrak' => (function () use ($kontrak) {
                    if (! $kontrak) {
                        return null;
                    }
                    $tglMulai = $kontrak->tgl_spmk ?? $kontrak->tgl_spk;
                    $tglSelesai = $kontrak->tglSelesaiBerjalan();
                    if (! $tglSelesai instanceof \Carbon\Carbon && $kontrak->tgl_selesai) {
                        $tglSelesai = $kontrak->tgl_selesai;
                    }

                    return [
                        // Mulai: SPMK → fallback SPK
                        'tgl_spmk' => $tglMulai?->format('Y-m-d'),
                        'tgl_spk' => $kontrak->tgl_spk?->format('Y-m-d'),
                        // Selesai: addendum disetujui bila ada
                        'tgl_selesai' => $tglSelesai instanceof \Carbon\Carbon
                            ? $tglSelesai->format('Y-m-d')
                            : null,
                        'spk' => $kontrak->spk,
                        'spmk' => $kontrak->spmk,
                        'nilai_kontrak' => $kontrak->nilaiKontrakBerjalan() ?? $kontrak->nilai_kontrak,
                    ];
                })(),
                'penyedia' => $penyedia ? [
                    'nama' => $penyedia->nama,
                    'direktur' => $penyedia->direktur,
                ] : null,
                'pengawas' => $pengawas ? [
                    'nama' => $pengawas->nama,
                    'nip' => $pengawas->nip,
                    'jabatan' => $pengawas->jabatan,
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
     * @OA\Post(
     *     path="/api/progress/{pekerjaanId}",
     *     summary="Store full progress report",
     *     tags={"Progress"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="pekerjaanId", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             @OA\Property(property="items", type="array", @OA\Items(type="object")),
     *             @OA\Property(property="week_count", type="integer", example=4)
     *         )
     *     ),
     *     @OA\Response(response=200, description="Progress saved")
     * )
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
