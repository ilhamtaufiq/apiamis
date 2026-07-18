<?php

namespace App\Http\Controllers;

use App\Models\Kontrak;
use App\Models\Pekerjaan;
use App\Models\Tiket;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Schema;

class DataQualityController extends Controller
{
    /**
     * Aggregate stats for dashboard cards.
     */
    public function getStats(Request $request)
    {
        $baseQuery = $this->basePekerjaanQuery($request);

        $noCoordsCount = (clone $baseQuery)->whereNotExists(function ($query) {
            $query->select(DB::raw(1))
                ->from('tbl_foto')
                ->whereRaw('tbl_foto.pekerjaan_id = tbl_pekerjaan.id')
                ->whereNotNull('koordinat');
        })->count();

        $noPhotosCount = (clone $baseQuery)->whereNotExists(function ($query) {
            $query->select(DB::raw(1))
                ->from('tbl_foto')
                ->whereRaw('tbl_foto.pekerjaan_id = tbl_pekerjaan.id');
        })->count();

        $startedNoPhotosCount = (clone $baseQuery)->whereExists(function ($query) {
            $query->select(DB::raw(1))
                ->from('tbl_kontrak')
                ->whereRaw('tbl_kontrak.id_pekerjaan = tbl_pekerjaan.id');
        })->whereNotExists(function ($query) {
            $query->select(DB::raw(1))
                ->from('tbl_foto')
                ->whereRaw('tbl_foto.pekerjaan_id = tbl_pekerjaan.id');
        })->count();

        $noContractCount = (clone $baseQuery)->whereNotExists(function ($query) {
            $query->select(DB::raw(1))
                ->from('tbl_kontrak')
                ->whereRaw('tbl_kontrak.id_pekerjaan = tbl_pekerjaan.id');
        })->count();

        return response()->json([
            'success' => true,
            'data' => [
                'no_coordinates' => $noCoordsCount,
                'no_photos' => $noPhotosCount,
                'started_no_photos' => $startedNoPhotosCount,
                'no_contracts' => $noContractCount,
                'total_jobs' => (clone $baseQuery)->count(),
            ],
        ]);
    }

    /**
     * Work-queue list of pekerjaan for a quality issue.
     *
     * issue: no_coordinates | no_photos | started_no_photos | no_contracts
     */
    public function getItems(Request $request)
    {
        $validated = $request->validate([
            'issue' => 'required|string|in:no_coordinates,no_photos,started_no_photos,no_contracts',
            'tahun' => 'nullable|integer',
            'search' => 'nullable|string|max:200',
            'per_page' => 'nullable|integer|min:1|max:100',
        ]);

        $query = $this->basePekerjaanQuery($request)
            ->with(['kecamatan:id,n_kec', 'desa:id,n_desa', 'pengawas:id,nama'])
            ->select('tbl_pekerjaan.*');

        $issue = $validated['issue'];

        if ($issue === 'no_coordinates') {
            $query->whereNotExists(function ($q) {
                $q->select(DB::raw(1))
                    ->from('tbl_foto')
                    ->whereRaw('tbl_foto.pekerjaan_id = tbl_pekerjaan.id')
                    ->whereNotNull('koordinat');
            });
        } elseif ($issue === 'no_photos') {
            $query->whereNotExists(function ($q) {
                $q->select(DB::raw(1))
                    ->from('tbl_foto')
                    ->whereRaw('tbl_foto.pekerjaan_id = tbl_pekerjaan.id');
            });
        } elseif ($issue === 'started_no_photos') {
            $query->whereExists(function ($q) {
                $q->select(DB::raw(1))
                    ->from('tbl_kontrak')
                    ->whereRaw('tbl_kontrak.id_pekerjaan = tbl_pekerjaan.id');
            })->whereNotExists(function ($q) {
                $q->select(DB::raw(1))
                    ->from('tbl_foto')
                    ->whereRaw('tbl_foto.pekerjaan_id = tbl_pekerjaan.id');
            });
        } else {
            $query->whereNotExists(function ($q) {
                $q->select(DB::raw(1))
                    ->from('tbl_kontrak')
                    ->whereRaw('tbl_kontrak.id_pekerjaan = tbl_pekerjaan.id');
            });
        }

        if (! empty($validated['search'])) {
            $search = $validated['search'];
            $query->where(function ($q) use ($search) {
                $q->where('nama_paket', 'like', "%{$search}%")
                    ->orWhere('kode_rekening', 'like', "%{$search}%");
            });
        }

        $paginator = $query->orderBy('nama_paket')->paginate($validated['per_page'] ?? 25);

        $items = collect($paginator->items())->map(function (Pekerjaan $job) use ($issue) {
            return [
                'id' => $job->id,
                'nama_paket' => $job->nama_paket,
                'kode_rekening' => $job->kode_rekening,
                'pagu' => $job->pagu,
                'kecamatan' => $job->kecamatan?->n_kec,
                'desa' => $job->desa?->n_desa,
                'pengawas' => $job->pengawas?->nama,
                'issue' => $issue,
                'href' => match ($issue) {
                    'no_coordinates' => "/pekerjaan/{$job->id}",
                    'no_photos', 'started_no_photos' => "/foto?pekerjaanId={$job->id}",
                    'no_contracts' => "/kontrak/new?pekerjaanId={$job->id}",
                    default => "/pekerjaan/{$job->id}",
                },
            ];
        });

        return response()->json([
            'success' => true,
            'data' => $items,
            'meta' => [
                'current_page' => $paginator->currentPage(),
                'last_page' => $paginator->lastPage(),
                'per_page' => $paginator->perPage(),
                'total' => $paginator->total(),
            ],
        ]);
    }

    /**
     * Unified "needs action" inbox for operators.
     *
     * Semua hitungan terkait pekerjaan mengabaikan paket status canceled/dibatalkan
     * (via basePekerjaanQuery / notCanceled, tiket, dan kontrak).
     */
    public function getActionInbox(Request $request)
    {
        $tahun = $request->query('tahun');
        $actions = [];

        // Stats sudah lewat basePekerjaanQuery()->notCanceled().
        $statsResponse = $this->getStats($request);
        $stats = $statsResponse->getData(true)['data'] ?? [];

        foreach ([
            'no_coordinates' => ['label' => 'Tanpa koordinat', 'severity' => 'high', 'href' => '/data-quality?issue=no_coordinates'],
            'started_no_photos' => ['label' => 'Berkontrak tanpa foto', 'severity' => 'high', 'href' => '/data-quality?issue=started_no_photos'],
            'no_photos' => ['label' => 'Tanpa foto', 'severity' => 'medium', 'href' => '/data-quality?issue=no_photos'],
            'no_contracts' => ['label' => 'Tanpa kontrak', 'severity' => 'medium', 'href' => '/data-quality?issue=no_contracts'],
        ] as $key => $meta) {
            $count = (int) ($stats[$key] ?? 0);
            if ($count > 0) {
                $actions[] = [
                    'id' => "dq-{$key}",
                    'source' => 'data_quality',
                    'title' => "{$count} pekerjaan {$meta['label']}",
                    'detail' => 'Perbaiki kelengkapan data pekerjaan (paket dibatalkan dikecualikan).',
                    'severity' => $meta['severity'],
                    'count' => $count,
                    'href' => $meta['href'].($tahun ? "&tahun={$tahun}" : ''),
                ];
            }
        }

        if (class_exists(Tiket::class) && Schema::hasTable('tbl_tiket')) {
            $openHigh = Tiket::query()
                ->whereIn('status', ['open', 'pending'])
                ->where('prioritas', 'high')
                ->where(function ($q) {
                    // Tiket tanpa pekerjaan tetap dihitung; yang terikat paket canceled di-skip.
                    $q->whereNull('pekerjaan_id')
                        ->orWhereHas('pekerjaan', fn ($pq) => $pq->notCanceled());
                })
                ->count();
            if ($openHigh > 0) {
                $actions[] = [
                    'id' => 'tiket-high',
                    'source' => 'tiket',
                    'title' => "{$openHigh} tiket prioritas tinggi terbuka",
                    'detail' => 'Perlu penanganan / eskalasi segera.',
                    'severity' => 'high',
                    'count' => $openHigh,
                    'href' => '/tiket',
                ];
            }

            $openAll = Tiket::query()
                ->whereIn('status', ['open', 'pending'])
                ->where(function ($q) {
                    $q->whereNull('pekerjaan_id')
                        ->orWhereHas('pekerjaan', fn ($pq) => $pq->notCanceled());
                })
                ->count();
            if ($openAll > 0) {
                $actions[] = [
                    'id' => 'tiket-open',
                    'source' => 'tiket',
                    'title' => "{$openAll} tiket masih terbuka",
                    'detail' => 'Termasuk pending dan open (paket dibatalkan dikecualikan).',
                    'severity' => $openAll > 20 ? 'medium' : 'low',
                    'count' => $openAll,
                    'href' => '/tiket',
                ];
            }
        }

        $endingSoon = Kontrak::query()
            ->whereNotNull('tgl_selesai')
            ->whereDate('tgl_selesai', '>=', now()->toDateString())
            ->whereDate('tgl_selesai', '<=', now()->addDays(30)->toDateString())
            // Abaikan kontrak milik paket dibatalkan.
            ->where(function ($q) {
                $q->whereDoesntHave('pekerjaan')
                    ->orWhereHas('pekerjaan', fn ($pq) => $pq->notCanceled());
            })
            ->when($tahun, function ($q) use ($tahun) {
                $q->where(function ($inner) use ($tahun) {
                    $inner->whereHas('pekerjaan.kegiatan', fn ($kq) => $kq->where('tahun_anggaran', $tahun))
                        ->orWhereHas('kegiatan', fn ($kq) => $kq->where('tahun_anggaran', $tahun));
                });
            })
            ->count();

        if ($endingSoon > 0) {
            $actions[] = [
                'id' => 'kontrak-h30',
                'source' => 'kontrak',
                'title' => "{$endingSoon} kontrak berakhir ≤ 30 hari",
                'detail' => 'Siapkan BA / addendum / perpanjangan bila perlu (paket dibatalkan dikecualikan).',
                'severity' => 'high',
                'count' => $endingSoon,
                'href' => '/kontrak',
            ];
        }

        usort($actions, function ($a, $b) {
            $rank = ['high' => 0, 'medium' => 1, 'low' => 2];

            return ($rank[$a['severity']] ?? 9) <=> ($rank[$b['severity']] ?? 9);
        });

        return response()->json([
            'success' => true,
            'data' => [
                'generated_at' => now()->toIso8601String(),
                'stats' => $stats,
                'actions' => $actions,
                'total_actions' => count($actions),
                'excludes_canceled_pekerjaan' => true,
            ],
        ]);
    }

    private function basePekerjaanQuery(Request $request)
    {
        $tahun = $request->query('tahun');
        $baseQuery = Pekerjaan::query()
            // Paket dibatalkan tidak perlu ditindaklanjuti (koordinat/foto/kontrak).
            ->notCanceled();

        if ($tahun) {
            $baseQuery->whereHas('kegiatan', function ($query) use ($tahun) {
                $query->where('tahun_anggaran', $tahun);
            });
        }

        return $baseQuery;
    }
}
