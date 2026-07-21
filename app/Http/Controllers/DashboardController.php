<?php

namespace App\Http\Controllers;

use App\Models\Kegiatan;
use Illuminate\Support\Facades\DB;
use Illuminate\Http\Request;

class DashboardController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/dashboard/stats",
     *     summary="Get dashboard statistics",
     *     tags={"Dashboard"},
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function stats(Request $request)
    {
        $tahun = $request->query('tahun');
        $user = auth()->user();
        
        // Bump key segment when stats payload / kontrak konsolidasi logic changes
        $version = \Illuminate\Support\Facades\Cache::get('dashboard_stats_version', 1);
        $cacheKey = "dashboard_stats_v{$version}_fk2_" . ($tahun ?? 'all') . "_" . ($user ? $user->id : 'guest');

        return \Illuminate\Support\Facades\Cache::remember($cacheKey, now()->addMinutes(30), function () use ($request, $tahun) {
            // Base query
            $query = Kegiatan::query();
            if ($tahun) {
                $query->where('tahun_anggaran', $tahun);
            }

            // Total kegiatan
            $totalKegiatan = (clone $query)->count();
            
            // Total pagu
            $totalPagu = (clone $query)->sum('pagu') ?? 0;
            
            $kegiatanPerTahun = (clone $query)->select('tahun_anggaran as name', DB::raw('count(*) as value'))
                ->groupBy('tahun_anggaran')
                ->orderBy('tahun_anggaran')
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => (string)$item->name ?? 'N/A',
                        'value' => $item->value
                    ];
                });
            
            // Kegiatan per sumber dana
            $kegiatanPerSumberDana = (clone $query)->select('sumber_dana as name', DB::raw('count(*) as value'))
                ->groupBy('sumber_dana')
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => $item->name ?? 'N/A',
                        'value' => $item->value
                    ];
                });
            
            // Pagu per tahun anggaran (dalam jutaan)
            $paguPerTahun = (clone $query)->select('tahun_anggaran as name', DB::raw('sum(pagu) / 1000000 as value'))
                ->groupBy('tahun_anggaran')
                ->orderBy('tahun_anggaran')
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => (string)$item->name ?? 'N/A',
                        'value' => round($item->value, 2)
                    ];
                });
            
            // Get available years for filter
            $availableYears = Kegiatan::select('tahun_anggaran')
                ->distinct()
                ->orderBy('tahun_anggaran', 'desc')
                ->pluck('tahun_anggaran');

            // Pekerjaan statistics (rekap status dulu, lalu hitung metrik utama tanpa canceled)
            $pekerjaanAllQuery = \App\Models\Pekerjaan::query();
            if ($tahun) {
                $pekerjaanAllQuery->whereHas('kegiatan', function ($q) use ($tahun) {
                    $q->where('tahun_anggaran', $tahun);
                });
            }

            $pekerjaanBatal = (clone $pekerjaanAllQuery)
                ->where('status', \App\Models\Pekerjaan::STATUS_CANCELED)
                ->count();
            $pekerjaanAktif = (clone $pekerjaanAllQuery)->notCanceled()->count();

            // withKontrak: legacy id_pekerjaan ATAU pivot konsolidasi kontrak_pekerjaan
            $pekerjaanBerkontrak = (clone $pekerjaanAllQuery)
                ->notCanceled()
                ->withKontrak()
                ->count();
            $pekerjaanBelumBerkontrak = max(0, $pekerjaanAktif - $pekerjaanBerkontrak);

            // Paket aktif: fisik vs konsultan (is_konsultan)
            $pekerjaanFisik = (clone $pekerjaanAllQuery)
                ->notCanceled()
                ->where(function ($q) {
                    $q->where('is_konsultan', false)->orWhereNull('is_konsultan');
                })
                ->count();
            $pekerjaanKonsultan = (clone $pekerjaanAllQuery)
                ->notCanceled()
                ->where('is_konsultan', true)
                ->count();
            $pekerjaanFisikBerkontrak = (clone $pekerjaanAllQuery)
                ->notCanceled()
                ->where(function ($q) {
                    $q->where('is_konsultan', false)->orWhereNull('is_konsultan');
                })
                ->withKontrak()
                ->count();
            $pekerjaanFisikBelumBerkontrak = max(0, $pekerjaanFisik - $pekerjaanFisikBerkontrak);

            // Metrik operasional hanya paket aktif (exclude dibatalkan)
            $pekerjaanQuery = (clone $pekerjaanAllQuery)->notCanceled();
            $pekerjaanFisikQuery = (clone $pekerjaanQuery)->where(function ($q) {
                $q->where('is_konsultan', false)->orWhereNull('is_konsultan');
            });
            $pekerjaanKonsultanQuery = (clone $pekerjaanQuery)->where('is_konsultan', true);

            $totalPekerjaan = $pekerjaanAktif;
            $totalPaguPekerjaan = (clone $pekerjaanQuery)->sum('pagu') ?? 0;
            $totalPaguPekerjaanFisik = (clone $pekerjaanFisikQuery)->sum('pagu') ?? 0;
            $totalPaguPekerjaanKonsultan = (clone $pekerjaanKonsultanQuery)->sum('pagu') ?? 0;
            
            // Pekerjaan per kecamatan
            $pekerjaanPerKecamatan = (clone $pekerjaanQuery)
                ->select('kecamatan_id', DB::raw('count(*) as value'))
                ->with('kecamatan:id,n_kec')
                ->groupBy('kecamatan_id')
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => $item->kecamatan->n_kec ?? 'N/A',
                        'value' => $item->value
                    ];
                });
            
            // Pekerjaan per desa (top 10)
            $pekerjaanPerDesa = (clone $pekerjaanQuery)
                ->select('desa_id', DB::raw('count(*) as value'))
                ->with('desa:id,n_desa')
                ->groupBy('desa_id')
                ->orderBy('value', 'desc')
                ->limit(10)
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => $item->desa->n_desa ?? 'N/A',
                        'value' => $item->value
                    ];
                });
            
            // Pagu pekerjaan per kecamatan (dalam jutaan)
            $paguPekerjaanPerKecamatan = (clone $pekerjaanQuery)
                ->select('kecamatan_id', DB::raw('sum(pagu) / 1000000 as value'))
                ->with('kecamatan:id,n_kec')
                ->groupBy('kecamatan_id')
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => $item->kecamatan->n_kec ?? 'N/A',
                        'value' => round($item->value, 2)
                    ];
                });

            // Kontrak: tautan legacy (id_pekerjaan) ATAU pivot multi-paket (konsolidasi)
            $pekerjaanAktifConstraint = function ($q) use ($tahun) {
                $q->notCanceled();
                if ($tahun) {
                    $q->whereHas('kegiatan', function ($kegiatanQuery) use ($tahun) {
                        $kegiatanQuery->where('tahun_anggaran', $tahun);
                    });
                }
            };
            $kontrakQuery = \App\Models\Kontrak::query()
                ->linkedToPekerjaan($pekerjaanAktifConstraint);

            $totalKontrak = (clone $kontrakQuery)->count();
            // nilai_kontrak dihitung per baris kontrak (1 kontrak konsolidasi = 1 nilai, tidak digandakan per paket)
            $totalNilaiKontrak = (clone $kontrakQuery)->sum('nilai_kontrak') ?? 0;
            
            // Kontrak per penyedia (top 10)
            $kontrakPerPenyedia = (clone $kontrakQuery)
                ->select('id_penyedia', DB::raw('count(*) as value'))
                ->with('penyedia:id,nama')
                ->groupBy('id_penyedia')
                ->orderBy('value', 'desc')
                ->limit(10)
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => $item->penyedia->nama ?? 'N/A',
                        'value' => $item->value
                    ];
                });
            
            // Nilai kontrak per penyedia (top 10, dalam jutaan)
            $nilaiKontrakPerPenyedia = (clone $kontrakQuery)
                ->select('id_penyedia', DB::raw('sum(nilai_kontrak) / 1000000 as value'))
                ->with('penyedia:id,nama')
                ->groupBy('id_penyedia')
                ->orderBy('value', 'desc')
                ->limit(10)
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => $item->penyedia->nama ?? 'N/A',
                        'value' => round($item->value ?? 0, 2)
                    ];
                });

            // Output statistics (hanya paket aktif)
            $outputQuery = \App\Models\Output::query()
                ->whereHas('pekerjaan', function ($q) use ($tahun) {
                    $q->notCanceled();
                    if ($tahun) {
                        $q->whereHas('kegiatan', function ($kegiatanQuery) use ($tahun) {
                            $kegiatanQuery->where('tahun_anggaran', $tahun);
                        });
                    }
                });

            $totalOutput = (clone $outputQuery)->count();

            // Output per satuan
            $outputPerSatuan = (clone $outputQuery)
                ->select('satuan as name', DB::raw('count(*) as value'))
                ->groupBy('satuan')
                ->orderBy('value', 'desc')
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => $item->name ?? 'N/A',
                        'value' => $item->value
                    ];
                });

            // Output per komponen
            $outputPerKomponen = (clone $outputQuery)
                ->select('komponen as name', DB::raw('count(*) as value'))
                ->groupBy('komponen')
                ->orderBy('value', 'desc')
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => $item->name ?? 'N/A',
                        'value' => $item->value
                    ];
                });

            // Penerima statistics (hanya paket aktif)
            $penerimaQuery = \App\Models\Penerima::query()
                ->whereHas('pekerjaan', function ($q) use ($tahun) {
                    $q->notCanceled();
                    if ($tahun) {
                        $q->whereHas('kegiatan', function ($kegiatanQuery) use ($tahun) {
                            $kegiatanQuery->where('tahun_anggaran', $tahun);
                        });
                    }
                });

            $totalPenerima = (clone $penerimaQuery)->count();
            $totalJiwa = (clone $penerimaQuery)->sum('jumlah_jiwa') ?? 0;

            // Penerima Komunal vs Individu
            $penerimaKomunalVsIndividu = (clone $penerimaQuery)
                ->select('is_komunal', DB::raw('count(*) as value'))
                ->groupBy('is_komunal')
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => $item->is_komunal ? 'Komunal' : 'Individu',
                        'value' => $item->value
                    ];
                });

            return response()->json([
                'data' => [
                    'totalKegiatan' => $totalKegiatan,
                    'totalPagu' => $totalPagu,
                    'kegiatanPerTahun' => $kegiatanPerTahun,
                    'kegiatanPerSumberDana' => $kegiatanPerSumberDana,
                    'paguPerTahun' => $paguPerTahun,
                    'availableYears' => $availableYears,
                    'totalPekerjaan' => $totalPekerjaan,
                    'totalPaguPekerjaan' => $totalPaguPekerjaan,
                    // Rekap status paket (aktif/batal/kontrak/fisik/konsultan) untuk executive brief
                    'pekerjaanAktif' => $pekerjaanAktif,
                    'pekerjaanBatal' => $pekerjaanBatal,
                    'pekerjaanBerkontrak' => $pekerjaanBerkontrak,
                    'pekerjaanBelumBerkontrak' => $pekerjaanBelumBerkontrak,
                    'pekerjaanFisik' => $pekerjaanFisik,
                    'pekerjaanKonsultan' => $pekerjaanKonsultan,
                    'pekerjaanFisikBerkontrak' => $pekerjaanFisikBerkontrak,
                    'pekerjaanFisikBelumBerkontrak' => $pekerjaanFisikBelumBerkontrak,
                    'totalPaguPekerjaanFisik' => $totalPaguPekerjaanFisik,
                    'totalPaguPekerjaanKonsultan' => $totalPaguPekerjaanKonsultan,
                    'pekerjaanPerKecamatan' => $pekerjaanPerKecamatan,
                    'pekerjaanPerDesa' => $pekerjaanPerDesa,
                    'paguPekerjaanPerKecamatan' => $paguPekerjaanPerKecamatan,
                    'totalKontrak' => $totalKontrak,
                    'totalNilaiKontrak' => $totalNilaiKontrak,
                    'kontrakPerPenyedia' => $kontrakPerPenyedia,
                    'nilaiKontrakPerPenyedia' => $nilaiKontrakPerPenyedia,
                    'totalOutput' => $totalOutput,
                    'outputPerSatuan' => $outputPerSatuan,
                    'outputPerKomponen' => $outputPerKomponen,
                    'totalPenerima' => $totalPenerima,
                    'totalJiwa' => $totalJiwa,
                    'penerimaKomunalVsIndividu' => $penerimaKomunalVsIndividu,
                ]
            ]);
        });
    }
}
