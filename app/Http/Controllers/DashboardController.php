<?php

namespace App\Http\Controllers;

use App\Models\Kegiatan;
use App\Models\Pekerjaan;
use App\Models\PekerjaanProgressEstimasiHistory;
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
        $kecamatanIds = $request->query('kecamatan_ids');
        if ($kecamatanIds && is_string($kecamatanIds)) {
            $kecamatanIds = explode(',', $kecamatanIds);
        }
        $kecamatanIds = $kecamatanIds
            ? array_map('intval', array_filter((array) $kecamatanIds, fn($v) => $v !== ''))
            : null;
        $user = auth()->user();

        // Bump key segment when stats payload / kontrak konsolidasi logic changes
        $version = \Illuminate\Support\Facades\Cache::get('dashboard_stats_version', 1);
        $cacheKey = "dashboard_stats_v{$version}_fk4_" . ($tahun ?? 'all')
            . "_k" . ($kecamatanIds ? implode('-', $kecamatanIds) : 'all')
            . "_" . ($user ? $user->id : 'guest');

        return \Illuminate\Support\Facades\Cache::remember($cacheKey, now()->addMinutes(30), function () use ($request, $tahun, $kecamatanIds) {
            // Base query
            $query = Kegiatan::query();
            if ($tahun) {
                $query->where('tahun_anggaran', $tahun);
            }
            if ($kecamatanIds) {
                $query->whereIn('id', function ($sub) use ($kecamatanIds) {
                    $sub->select('kegiatan_id')
                        ->from('tbl_pekerjaan')
                        ->whereIn('kecamatan_id', $kecamatanIds);
                });
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

            // Pekerjaan aktif (exclude dibatalkan) — dipakai sub kegiatan, metrik, dan rekap di bawah.
            $pekerjaanQuery = Pekerjaan::query()->notCanceled();
            if ($tahun) {
                $pekerjaanQuery->whereHas('kegiatan', fn($q) => $q->where('tahun_anggaran', $tahun));
            }
            if ($kecamatanIds) {
                $pekerjaanQuery->whereIn('kecamatan_id', $kecamatanIds);
            }

            // All pekerjaan query (include canceled) — dipakai untuk rekap batal & belum kontrak per sub kegiatan
            $pekerjaanAllQuery = Pekerjaan::query();
            if ($tahun) {
                $pekerjaanAllQuery->whereHas('kegiatan', fn($q) => $q->where('tahun_anggaran', $tahun));
            }
            if ($kecamatanIds) {
                $pekerjaanAllQuery->whereIn('kecamatan_id', $kecamatanIds);
            }

            // Sub Kegiatan stats: pagu dihitung dari pekerjaan aktif sudah berkontrak (bukan pagu kegiatan),
            // progress = rata-rata progress berbobot paket di sub kegiatan tsb.
            // Exclude batal & belum berkontrak.
            $subKegiatanRows = (clone $pekerjaanQuery)->withKontrak()
                ->select('tbl_kegiatan.nama_sub_kegiatan as name', DB::raw('count(*) as count'), DB::raw('sum(tbl_pekerjaan.pagu) as pagu'))
                ->join('tbl_kegiatan', 'tbl_pekerjaan.kegiatan_id', '=', 'tbl_kegiatan.id')
                ->whereNotNull('tbl_kegiatan.nama_sub_kegiatan')
                ->where('tbl_kegiatan.nama_sub_kegiatan', '!=', '')
                ->groupBy('tbl_kegiatan.nama_sub_kegiatan')
                ->get();

            // Batal per sub kegiatan
            $batalBySub = (clone $pekerjaanAllQuery)
                ->where('status', \App\Models\Pekerjaan::STATUS_CANCELED)
                ->join('tbl_kegiatan', 'tbl_pekerjaan.kegiatan_id', '=', 'tbl_kegiatan.id')
                ->whereNotNull('tbl_kegiatan.nama_sub_kegiatan')
                ->select('tbl_kegiatan.nama_sub_kegiatan as name', DB::raw('count(*) as batal'))
                ->groupBy('tbl_kegiatan.nama_sub_kegiatan')
                ->pluck('batal', 'name');

            // Belum berkontrak per sub kegiatan (dari paket aktif saja, pakai scope withKontrak)
            $belumBerkontrakBySub = (clone $pekerjaanAllQuery)
                ->notCanceled()
                ->whereDoesntHave('kontraks')
                ->join('tbl_kegiatan', 'tbl_pekerjaan.kegiatan_id', '=', 'tbl_kegiatan.id')
                ->whereNotNull('tbl_kegiatan.nama_sub_kegiatan')
                ->select('tbl_kegiatan.nama_sub_kegiatan as name', DB::raw('count(*) as belum'))
                ->groupBy('tbl_kegiatan.nama_sub_kegiatan')
                ->pluck('belum', 'name');

            // Progress & SP2D per sub kegiatan dari PekerjaanProgressEstimasiHistory
            $subKegiatanPekerjaan = (clone $pekerjaanQuery)
                ->select('tbl_pekerjaan.id', 'tbl_pekerjaan.kegiatan_id')
                ->with('kegiatan:id,nama_sub_kegiatan')
                ->get();

            $progressAcc = [];
            $sp2dAcc = [];
            $kontrakAcc = [];
            $pekerjaanIdsSub = $subKegiatanPekerjaan->pluck('id')->toArray();

            if (!empty($pekerjaanIdsSub)) {
                // Latest realisasi fisik per pekerjaan (persen)
                $latestFisik = PekerjaanProgressEstimasiHistory::query()
                    ->whereIn('pekerjaan_id', $pekerjaanIdsSub)
                    ->where('tipe', 'realisasi')
                    ->where('jenis', 'fisik')
                    ->orderBy('tanggal', 'desc')
                    ->orderBy('id', 'desc')
                    ->get()
                    ->keyBy('pekerjaan_id');

                // Total SP2D keuangan per pekerjaan (nilai asli)
                $sp2dPerPekerjaan = PekerjaanProgressEstimasiHistory::query()
                    ->whereIn('pekerjaan_id', $pekerjaanIdsSub)
                    ->where('tipe', 'realisasi')
                    ->where('jenis', 'keuangan')
                    ->select('pekerjaan_id', DB::raw('sum(nilai) as total_sp2d'))
                    ->groupBy('pekerjaan_id')
                    ->pluck('total_sp2d', 'pekerjaan_id');

                foreach ($subKegiatanPekerjaan as $p) {
                    $name = $p->kegiatan?->nama_sub_kegiatan;
                    if (!$name) continue;

                    // Progress dari estimasi realisasi fisik
                    $fisik = $latestFisik[$p->id] ?? null;
                    if ($fisik) {
                        $progressAcc[$name] ??= ['sum' => 0.0, 'n' => 0];
                        $progressAcc[$name]['sum'] += (float) $fisik->persen;
                        $progressAcc[$name]['n']++;
                    }

                    // SP2D dari realisasi keuangan
                    $sp2d = (float) ($sp2dPerPekerjaan[$p->id] ?? 0);
                    if ($sp2d > 0) {
                        $sp2dAcc[$name] = ($sp2dAcc[$name] ?? 0) + $sp2d;
                    }
                }

                // Nilai kontrak per sub kegiatan (distinct per kontrak; 1 kontrak
                // konsolidasi multi-paket dihitung penuh di tiap sub terkait).
                // Mendukung tautan legacy (tbl_kontrak.id_pekerjaan) + pivot.
                $legacyLinks = DB::table('tbl_kontrak')
                    ->join('tbl_pekerjaan', 'tbl_pekerjaan.id', '=', 'tbl_kontrak.id_pekerjaan')
                    ->join('tbl_kegiatan', 'tbl_kegiatan.id', '=', 'tbl_pekerjaan.kegiatan_id')
                    ->whereIn('tbl_kontrak.id_pekerjaan', $pekerjaanIdsSub)
                    ->whereNotNull('tbl_kegiatan.nama_sub_kegiatan')
                    ->select('tbl_kontrak.id as kid', 'tbl_kegiatan.nama_sub_kegiatan as name')
                    ->get();
                $pivotLinks = DB::table('kontrak_pekerjaan')
                    ->join('tbl_pekerjaan', 'tbl_pekerjaan.id', '=', 'kontrak_pekerjaan.pekerjaan_id')
                    ->join('tbl_kegiatan', 'tbl_kegiatan.id', '=', 'tbl_pekerjaan.kegiatan_id')
                    ->whereIn('kontrak_pekerjaan.pekerjaan_id', $pekerjaanIdsSub)
                    ->whereNotNull('tbl_kegiatan.nama_sub_kegiatan')
                    ->select('kontrak_pekerjaan.kontrak_id as kid', 'tbl_kegiatan.nama_sub_kegiatan as name')
                    ->get();
                $kontrakIdsBySub = [];
                foreach ([$legacyLinks, $pivotLinks] as $rows) {
                    foreach ($rows as $row) {
                        if ($row->kid === null) continue;
                        $kontrakIdsBySub[$row->name][$row->kid] = true;
                    }
                }
                if (!empty($kontrakIdsBySub)) {
                    $allKontrakIds = collect($kontrakIdsBySub)
                        ->flatMap(fn($set) => array_keys($set))
                        ->unique()
                        ->values()
                        ->toArray();
                    $nilaiByKontrakId = DB::table('tbl_kontrak')
                        ->whereIn('id', $allKontrakIds)
                        ->pluck('nilai_kontrak', 'id');
                    foreach ($kontrakIdsBySub as $name => $set) {
                        $sum = 0.0;
                        foreach (array_keys($set) as $kid) {
                            $sum += (float) ($nilaiByKontrakId[$kid] ?? 0);
                        }
                        $kontrakAcc[$name] = $sum;
                    }
                }
            }

            $subKegiatanStats = $subKegiatanRows->map(function ($item) use ($progressAcc, $sp2dAcc, $kontrakAcc, $batalBySub, $belumBerkontrakBySub) {
                $acc = $progressAcc[$item->name] ?? null;
                return [
                    'name' => $item->name,
                    'count' => (int) $item->count,
                    'paguM' => round((float) $item->pagu / 1000000, 2),
                    'progress' => $acc ? round($acc['sum'] / $acc['n'], 1) : 0,
                    'hasProgress' => $acc !== null,
                    'sp2dTotal' => round($sp2dAcc[$item->name] ?? 0),
                    'kontrakTotal' => round($kontrakAcc[$item->name] ?? 0),
                    'batal' => (int) ($batalBySub[$item->name] ?? 0),
                    'belumBerkontrak' => (int) ($belumBerkontrakBySub[$item->name] ?? 0),
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
            
            // Pekerjaan per desa + pagu asli per desa
            $pekerjaanPerDesa = (clone $pekerjaanQuery)
                ->select(
                    'desa_id',
                    DB::raw('count(*) as value'),
                    DB::raw('sum(pagu) / 1000000 as paguJt')
                )
                ->with('desa:id,n_desa')
                ->groupBy('desa_id')
                ->orderBy('value', 'desc')
                ->get()
                ->map(function ($item) {
                    return [
                        'name' => $item->desa->n_desa ?? 'N/A',
                        'value' => (int) $item->value,
                        'paguJt' => round((float) $item->paguJt, 2),
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
                    'subKegiatanStats' => $subKegiatanStats,
                    'paguPerTahun' => $paguPerTahun,
                    'availableYears' => $availableYears,
                    'totalPekerjaan' => $totalPekerjaan,
                    'totalPaguPekerjaan' => $totalPaguPekerjaan,
                    // Rekap status paket (aktif/batal/kontrak/fisik/konsultan) untuk executive brief
                    'pekerjaanAktif' => $pekerjaanAktif,
                    'pekerjaanBatal' => $pekerjaanBatal,
                    'pekerjaanBerkontrak' => $pekerjaanBerkontrak,
                    'pekerjaanBelumBerkontrak' => $pekerjaanBelumBerkontrak,
                    'pekerjaanBatal' => $pekerjaanBatal,
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

    /**
     * Monthly trend: fisik average % + keuangan nominal from SP2D progress
     */
    public function executiveProgress(Request $request)
    {
        $tahun = (int) ($request->query('tahun') ?? date('Y'));
        $pekerjaanIds = $request->query('pekerjaan_ids');
        $kecamatanIds = $request->query('kecamatan_ids');
        if ($kecamatanIds && is_string($kecamatanIds)) {
            $kecamatanIds = explode(',', $kecamatanIds);
        }
        $kecamatanIds = $kecamatanIds
            ? array_map('intval', array_filter((array) $kecamatanIds, fn($v) => $v !== ''))
            : null;

        // Active pekerjaan for tahun (exclude canceled)
        $basePekerjaanQuery = Pekerjaan::notCanceled()
            ->whereHas('kegiatan', fn($q) => $q->where('tahun_anggaran', $tahun));
        if ($kecamatanIds) {
            $basePekerjaanQuery->whereIn('kecamatan_id', $kecamatanIds);
        }

        if ($pekerjaanIds) {
            $activeIds = $basePekerjaanQuery->pluck('id')->map('intval')->toArray();
            $ids = array_values(array_intersect(array_map('intval', explode(',', $pekerjaanIds)), $activeIds));
            if (empty($ids)) {
                return response()->json([
                    'success' => true,
                    'data' => ['monthly_trend' => [], 'totals' => ['keuangan_total' => 0]]
                ]);
            }
        } else {
            $ids = $basePekerjaanQuery->pluck('id')->map('intval')->toArray();
        }

        if (empty($ids)) {
            return response()->json([
                'success' => true,
                'data' => ['monthly_trend' => [], 'totals' => ['keuangan_total' => 0]]
            ]);
        }

        $monthNames = [
            1 => 'Jan', 2 => 'Feb', 3 => 'Mar', 4 => 'Apr',
            5 => 'Mei', 6 => 'Jun', 7 => 'Jul', 8 => 'Agu',
            9 => 'Sep', 10 => 'Okt', 11 => 'Nov', 12 => 'Des',
        ];

        // Semua entri estimasi (rencana + realisasi) tahun ini, terbaru dulu
        $historyRecords = PekerjaanProgressEstimasiHistory::query()
            ->whereIn('pekerjaan_id', $ids)
            ->whereRaw('YEAR(tanggal) = ?', [$tahun])
            ->orderBy('tanggal', 'desc')
            ->orderBy('id', 'desc')
            ->get();

        $paguByPekerjaan = Pekerjaan::whereIn('id', $ids)->pluck('pagu', 'id');

        // 1. Realisasi fisik: latest persen per (pekerjaan, bulan) — carry forward
        // 2. Rencana fisik: rencana% per (pekerjaan, bulan)
        // 3. Keuangan: nominal SP2D asli (kolom `nilai`) dari realisasi/keuangan
        $latestFisik = [];     // pid => [month => persen]
        $latestRencana = [];   // pid => [month => persen]
        $sp2dByMonth = [];     // month => sum of nilai

        foreach ($historyRecords as $r) {
            $pid = (int) $r->pekerjaan_id;
            $m = (int) $r->tanggal->month;

            if ($r->tipe === 'realisasi' && $r->jenis === 'fisik') {
                $latestFisik[$pid][$m] ??= (float) $r->persen;
            } elseif ($r->tipe === 'rencana' && $r->jenis === 'fisik') {
                $latestRencana[$pid][$m] ??= (float) $r->persen;
            } elseif ($r->tipe === 'realisasi' && $r->jenis === 'keuangan') {
                // Ambil nilai SP2D asli, bukan estimasi (delta% × pagu)
                $sp2dByMonth[$m] = ($sp2dByMonth[$m] ?? 0) + (float) ($r->nilai ?? 0);
            }
        }

        // Aggregate fisik: carry forward per-paket
        $months = range(1, 12);
        $fisikSum = array_fill(1, 12, 0.0);
        $rencanaSum = array_fill(1, 12, 0.0);
        $fisikCount = array_fill(1, 12, 0);
        $rencanaCount = array_fill(1, 12, 0);

        foreach ($latestFisik as $pid => $byMonth) {
            $prev = null;
            foreach ($months as $m) {
                if (isset($byMonth[$m])) $prev = $byMonth[$m];
                if ($prev !== null) {
                    $fisikSum[$m] += $prev;
                    $fisikCount[$m]++;
                }
            }
        }
        foreach ($latestRencana as $pid => $byMonth) {
            $prev = null;
            foreach ($months as $m) {
                if (isset($byMonth[$m])) $prev = $byMonth[$m];
                if ($prev !== null) {
                    $rencanaSum[$m] += $prev;
                    $rencanaCount[$m]++;
                }
            }
        }

        $totalFisikJobs = count($latestFisik);

        $monthlyTrend = [];
        foreach ($months as $m) {
            $monthlyTrend[] = [
                'month' => $monthNames[$m] ?? "B{$m}",
                'fisik_avg'  => $fisikCount[$m] > 0 ? round($fisikSum[$m] / $fisikCount[$m], 1) : 0,
                'rencana_avg' => $rencanaCount[$m] > 0 ? round($rencanaSum[$m] / $rencanaCount[$m], 1) : 0,
                // nominal SP2D asli dari tabel history (bukan estimasi)
                'keuangan_sum' => round($sp2dByMonth[$m] ?? 0),
            ];
        }

        return response()->json([
            'success' => true,
            'data' => [
                'monthly_trend' => $monthlyTrend,
                'totals' => [
                    // Total SP2D asli tahun berjalan
                    'keuangan_total' => round(array_sum($sp2dByMonth)),
                ],
            ],
        ]);
    }

    /**
     * Progress berbobot 0–100 dari content JSON tbl_progress.
     * Return null kalau paket belum punya item valid (biar tidak dihitung sebagai 0%).
     */
    private static function weightedProgressFromContent(?array $content): ?float
    {
        $items = $content['items'] ?? [];
        if (!$items) {
            return null;
        }

        $weighted = 0.0;
        $weight = 0.0;

        foreach ($items as $item) {
            $bobot = (float) ($item['bobot'] ?? 0);
            $targetVolume = (float) ($item['target_volume'] ?? 0);
            if ($bobot <= 0 || $targetVolume <= 0) {
                continue;
            }

            $realisasi = 0.0;
            foreach ($item['weekly_data'] ?? [] as $week) {
                $realisasi += (float) ($week['realisasi'] ?? 0);
            }

            $weighted += ($realisasi / $targetVolume) * $bobot;
            $weight += $bobot;
        }

        if ($weight <= 0) {
            return null;
        }

        // Normalisasi kalau total bobot tidak genap 100.
        return min(100.0, $weighted * (100.0 / $weight));
    }
}
