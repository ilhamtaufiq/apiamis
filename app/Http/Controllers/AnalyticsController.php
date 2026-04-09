<?php

namespace App\Http\Controllers;

use App\Models\Pekerjaan;
use App\Models\Progress;
use App\Models\Kecamatan;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Http\JsonResponse;

class AnalyticsController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/analytics/stats",
     *     summary="Get analytics charts and statistics data",
     *     tags={"Analytics"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="kecamatan_ids", in="query", required=false, @OA\Schema(type="string"), description="Comma separated IDs"),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function stats(Request $request): JsonResponse
    {
        $tahun = $request->query('tahun');
        $kecamatanIds = $request->query('kecamatan_ids'); // Array or comma separated
        if ($kecamatanIds && is_string($kecamatanIds)) {
            $kecamatanIds = explode(',', $kecamatanIds);
        }

        // Base Pekerjaan Query
        $pekerjaanQuery = Pekerjaan::query();
        if ($tahun) {
            $pekerjaanQuery->whereHas('kegiatan', function ($q) use ($tahun) {
                $q->where('tahun_anggaran', $tahun);
            });
        }
        if ($kecamatanIds) {
            $pekerjaanQuery->whereIn('kecamatan_id', $kecamatanIds);
        }

        $pekerjaanIds = (clone $pekerjaanQuery)->pluck('id');
        
        // Get all progress for these pekerjaan
        $allProgress = Progress::whereIn('pekerjaan_id', $pekerjaanIds)->get();

        // 1. Trend Calculation (Weekly Cumulative)
        $trend = [];
        $maxWeeks = 0;
        
        // First determine max weeks
        foreach ($allProgress as $p) {
            $content = $p->content;
            if (isset($content['week_count'])) {
                $maxWeeks = max($maxWeeks, (int)$content['week_count']);
            }
        }
        
        if ($maxWeeks == 0) $maxWeeks = 12; // Fallback

        for ($w = 1; $w <= $maxWeeks; $w++) {
            $sumRenc = 0;
            $sumReal = 0;
            $count = 0;

            foreach ($allProgress as $p) {
                $content = $p->content;
                $items = $content['items'] ?? [];
                
                $projectRenc = 0;
                $projectReal = 0;
                
                foreach ($items as $item) {
                    $bobot = (float)($item['bobot'] ?? 0);
                    $targetVol = (float)($item['target_volume'] ?? 0);
                    if ($targetVol <= 0) continue;

                    $itemRenc = 0;
                    $itemReal = 0;
                    $weeklyData = $item['weekly_data'] ?? [];
                    
                    for ($iw = 1; $iw <= $w; $iw++) {
                        if (isset($weeklyData[$iw])) {
                            $itemRenc += (float)($weeklyData[$iw]['rencana'] ?? 0);
                            $itemReal += (float)($weeklyData[$iw]['realisasi'] ?? 0);
                        }
                    }

                    $projectRenc += ($itemRenc / $targetVol) * $bobot;
                    $projectReal += ($itemReal / $targetVol) * $bobot;
                }
                
                $sumRenc += $projectRenc;
                $sumReal += $projectReal;
                $count++;
            }

            $trend[] = [
                'week' => "M$w",
                'rencana' => $count > 0 ? round($sumRenc / $count, 2) : 0,
                'realisasi' => $count > 0 ? round($sumReal / $count, 2) : 0,
            ];
        }

        // 2. Regional Performance
        $regions = Kecamatan::select('id', 'n_kec')
            ->get()
            ->map(function ($kec) use ($allProgress, $pekerjaanQuery, $tahun) {
                // Get jobs for this kecamatan
                $jobIds = (clone $pekerjaanQuery)->where('kecamatan_id', $kec->id)->pluck('id');
                $kecProgress = $allProgress->whereIn('pekerjaan_id', $jobIds);
                
                $totalProgress = 0;
                $count = 0;

                foreach ($kecProgress as $p) {
                    $content = $p->content;
                    $items = $content['items'] ?? [];
                    $projectProgress = 0;
                    
                    foreach ($items as $item) {
                        $bobot = (float)($item['bobot'] ?? 0);
                        $targetVol = (float)($item['target_volume'] ?? 0);
                        if ($targetVol <= 0) continue;

                        $totalReal = 0;
                        $weeklyData = $item['weekly_data'] ?? [];
                        foreach ($weeklyData as $wData) {
                            $totalReal += (float)($wData['realisasi'] ?? 0);
                        }
                        
                        $projectProgress += ($totalReal / $targetVol) * $bobot;
                    }
                    $totalProgress += $projectProgress;
                    $count++;
                }

                return [
                    'name' => $kec->n_kec,
                    'value' => $count > 0 ? round($totalProgress / $count, 2) : 0,
                ];
            })
            ->filter(fn($r) => $r['value'] > 0)
            ->values();

        // 3. Category distribution (e.g. by Sumber Dana)
        $categories = (clone $pekerjaanQuery)
            ->join('tbl_kegiatan', 'tbl_pekerjaan.kegiatan_id', '=', 'tbl_kegiatan.id')
            ->select('tbl_kegiatan.sumber_dana as name', DB::raw('count(*) as value'))
            ->groupBy('tbl_kegiatan.sumber_dana')
            ->get();

        return response()->json([
            'success' => true,
            'data' => [
                'trend' => $trend,
                'regions' => $regions,
                'categories' => $categories,
            ]
        ]);
    }
}
