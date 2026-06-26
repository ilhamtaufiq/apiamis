<?php

namespace App\Services;

use App\Models\Desa;
use App\Models\Pekerjaan;
use App\Models\SpamAchievement;
use App\Models\SpamBudget;
use App\Models\UnitSpam;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\DB;

class SpamPekerjaanIntegrationService
{
    public function manualCapTahun(): string
    {
        return (string) ((int) date('Y') - 1);
    }

    public function manualScopeLabel(?string $tahun = null): string
    {
        if ($tahun) {
            return "Tahun {$tahun}";
        }

        return 's/d tahun '.$this->manualCapTahun();
    }

    public function airMinumQuery(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null,
        ?string $search = null
    ): Builder {
        $query = Pekerjaan::query()
            ->byUserRole()
            ->whereHas('kegiatan', function (Builder $q) {
                $q->where('sub_bidang', 'Air Minum');
            });

        if ($tahun) {
            $query->whereHas('kegiatan', function (Builder $q) use ($tahun) {
                $q->where('tahun_anggaran', $tahun);
            });
        }

        if ($kecamatanId) {
            $query->where('kecamatan_id', $kecamatanId);
        }

        if ($desaId) {
            $query->where('desa_id', $desaId);
        }

        if ($search) {
            $term = '%'.$search.'%';
            $query->where(function (Builder $q) use ($term) {
                $q->where('nama_paket', 'like', $term)
                    ->orWhereHas('desa', fn (Builder $dq) => $dq->where('n_desa', 'like', $term))
                    ->orWhereHas('kecamatan', fn (Builder $kq) => $kq->where('n_kec', 'like', $term));
            });
        }

        return $query;
    }

    /**
     * @return array{sr: int, kk: int, jiwa: int, nilai_kontrak: float, progress_total: float}
     */
    public function derivedMetricsForPekerjaan(Pekerjaan $pekerjaan): array
    {
        $pekerjaan->loadMissing(['output', 'penerima', 'kontrak', 'progress']);

        $sr = 0;
        foreach ($pekerjaan->output as $output) {
            if (stripos((string) $output->komponen, 'sambungan rumah') !== false) {
                $sr += (int) round((float) $output->volume);
            }
        }

        $kk = (int) ($pekerjaan->penerima_count ?? $pekerjaan->penerima->count());
        $jiwaSum = (int) $pekerjaan->penerima->sum('jumlah_jiwa');
        $jiwa = $jiwaSum > 0 ? $jiwaSum : ($kk * 5);

        $nilaiKontrak = (float) $pekerjaan->kontrak->sum('nilai_kontrak');
        $progressTotal = $this->calculateProgressTotal($pekerjaan->progress?->content ?? []);

        return [
            'sr' => $sr,
            'kk' => $kk,
            'jiwa' => $jiwa,
            'nilai_kontrak' => $nilaiKontrak,
            'progress_total' => $progressTotal,
        ];
    }

    /**
     * @param  Collection<int, Pekerjaan>|array<int, Pekerjaan>  $pekerjaanCollection
     * @return array{sr: int, kk: int, jiwa: int, nilai_kontrak: float, progress_avg: float}
     */
    public function aggregateDerived(Collection|array $pekerjaanCollection): array
    {
        $collection = $pekerjaanCollection instanceof Collection
            ? $pekerjaanCollection
            : collect($pekerjaanCollection);

        $sr = 0;
        $kk = 0;
        $jiwa = 0;
        $nilaiKontrak = 0.0;
        $progressValues = [];

        foreach ($collection as $pekerjaan) {
            $metrics = $this->derivedMetricsForPekerjaan($pekerjaan);
            $sr += $metrics['sr'];
            $kk += $metrics['kk'];
            $jiwa += $metrics['jiwa'];
            $nilaiKontrak += $metrics['nilai_kontrak'];
            $progressValues[] = $metrics['progress_total'];
        }

        return [
            'sr' => $sr,
            'kk' => $kk,
            'jiwa' => $jiwa,
            'nilai_kontrak' => $nilaiKontrak,
            'progress_avg' => count($progressValues) > 0
                ? round(array_sum($progressValues) / count($progressValues), 1)
                : 0.0,
        ];
    }

    /**
     * @return array{sr: int, kk: int, jiwa: int, nilai_kontrak: float}
     */
    public function aggregateManualForDesa(int $desaId, ?string $tahun = null, ?Collection $units = null): array
    {
        $units = $units ?? UnitSpam::query()->where('desa_id', $desaId)->get();
        $unitIds = $units->pluck('id');

        if ($unitIds->isEmpty()) {
            return [
                'sr' => 0,
                'kk' => 0,
                'jiwa' => 0,
                'nilai_kontrak' => 0.0,
            ];
        }

        $achievementQuery = SpamAchievement::query()->whereIn('unit_spam_id', $unitIds);
        $budgetQuery = SpamBudget::query()->whereIn('unit_spam_id', $unitIds);

        if ($tahun) {
            $achievementQuery->where('tahun', $tahun);
            $budgetQuery->where('tahun', $tahun);
        } else {
            $capTahun = $this->manualCapTahun();
            $achievementQuery->where('tahun', '<=', $capTahun);
            $budgetQuery->where('tahun', '<=', $capTahun);
        }

        return [
            'sr' => (int) $achievementQuery->sum('jumlah_sr'),
            'kk' => (int) $achievementQuery->sum('jumlah_kk'),
            'jiwa' => (int) $achievementQuery->sum('jumlah_jiwa'),
            'nilai_kontrak' => (float) $budgetQuery->sum('nilai_kontrak'),
        ];
    }

    /**
     * @return array{sr: int, kk: int, jiwa: int, nilai_kontrak: float}
     */
    public function aggregateManualGlobal(?string $tahun = null, ?int $kecamatanId = null): array
    {
        $achievementQuery = SpamAchievement::query();
        $budgetQuery = SpamBudget::query();

        if ($tahun) {
            $achievementQuery->where('tahun', $tahun);
            $budgetQuery->where('tahun', $tahun);
        } else {
            $capTahun = $this->manualCapTahun();
            $achievementQuery->where('tahun', '<=', $capTahun);
            $budgetQuery->where('tahun', '<=', $capTahun);
        }

        if ($kecamatanId) {
            $achievementQuery->whereHas('unitSpam.desa', fn ($q) => $q->where('kecamatan_id', $kecamatanId));
            $budgetQuery->whereHas('unitSpam.desa', fn ($q) => $q->where('kecamatan_id', $kecamatanId));
        }

        return [
            'sr' => (int) $achievementQuery->sum('jumlah_sr'),
            'kk' => (int) $achievementQuery->sum('jumlah_kk'),
            'jiwa' => (int) $achievementQuery->sum('jumlah_jiwa'),
            'nilai_kontrak' => (float) $budgetQuery->sum('nilai_kontrak'),
        ];
    }

    public function buildDesaIntegrationRow(Desa $desa, ?string $tahun = null): array
    {
        $desa->loadMissing('kecamatan');

        $units = UnitSpam::query()
            ->with('pengelola')
            ->where('desa_id', $desa->id)
            ->get();

        $pekerjaan = $this->airMinumQuery($tahun, null, $desa->id, null)
            ->with(['kegiatan', 'output', 'penerima', 'kontrak', 'progress'])
            ->withCount(['penerima', 'foto'])
            ->get();

        $derived = $this->aggregateDerived($pekerjaan);
        $manual = $this->aggregateManualForDesa($desa->id, $tahun, $units);

        return [
            'desa' => [
                'id' => $desa->id,
                'n_desa' => $desa->n_desa,
                'target' => $desa->target,
                'bjp_master' => $desa->bjp_master,
                'kecamatan' => [
                    'id' => $desa->kecamatan->id,
                    'n_kec' => $desa->kecamatan->n_kec,
                ],
            ],
            'units' => $units->map(fn (UnitSpam $unit) => [
                'id' => $unit->id,
                'name' => $unit->name,
                'is_simspam' => (bool) $unit->is_simspam,
                'sistem_layanan' => $unit->sistem_layanan,
                'pokmas' => $unit->pengelola?->pokmas,
                'kepala' => $unit->pengelola?->kepala,
            ])->values()->all(),
            'unit_count' => $units->count(),
            'pekerjaan_count' => $pekerjaan->count(),
            'pekerjaan' => $pekerjaan->map(fn (Pekerjaan $item) => $this->formatIntegrationPekerjaan($item))->values()->all(),
            'derived' => $derived,
            'manual' => $manual,
            'sync_status' => $this->resolveSyncStatus($units->count(), $pekerjaan->count(), $derived, $manual),
        ];
    }

    /**
     * @return array{data: array<int, array>, meta: array<string, int>, summary: array<string, int>}
     */
    public function paginateIntegration(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null,
        ?string $search = null,
        ?string $syncStatus = null,
        int $perPage = 15,
        int $page = 1
    ): array {
        $rows = $this->integrationDesaQuery($tahun, $kecamatanId, $desaId, $search)
            ->with('kecamatan')
            ->orderBy('n_desa')
            ->get()
            ->map(fn (Desa $desa) => $this->buildDesaIntegrationRow($desa, $tahun));

        if ($syncStatus) {
            $rows = $rows->filter(fn (array $row) => $row['sync_status'] === $syncStatus);
        }

        $total = $rows->count();
        $lastPage = max(1, (int) ceil($total / $perPage));
        $page = min(max(1, $page), $lastPage);
        $offset = ($page - 1) * $perPage;

        $paginatedRows = $rows
            ->slice($offset, $perPage)
            ->values()
            ->all();

        return [
            'data' => $paginatedRows,
            'meta' => [
                'current_page' => $page,
                'last_page' => $lastPage,
                'per_page' => $perPage,
                'total' => $total,
            ],
            'summary' => $this->summarizeRows($paginatedRows),
        ];
    }

    /**
     * @return array<string, int|float>
     */
    public function integrationSummary(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null
    ): array {
        $desas = $this->integrationDesaQuery($tahun, $kecamatanId, $desaId, null)
            ->with('kecamatan')
            ->orderBy('n_desa')
            ->get();

        $rows = $desas
            ->map(fn (Desa $desa) => $this->buildDesaIntegrationRow($desa, $tahun))
            ->all();

        $summary = $this->summarizeRows($rows);

        $derivedTotals = [
            'sr' => 0,
            'kk' => 0,
            'jiwa' => 0,
            'nilai_kontrak' => 0.0,
        ];

        foreach ($rows as $row) {
            $derivedTotals['sr'] += $row['derived']['sr'];
            $derivedTotals['kk'] += $row['derived']['kk'];
            $derivedTotals['jiwa'] += $row['derived']['jiwa'];
            $derivedTotals['nilai_kontrak'] += $row['derived']['nilai_kontrak'];
        }

        $manualTotals = $this->aggregateManualGlobal($tahun, $kecamatanId);

        return array_merge($summary, [
            'pekerjaan_air_minum_count' => (int) $this->airMinumQuery($tahun, $kecamatanId, $desaId)->count(),
            'derived_sr' => $derivedTotals['sr'],
            'derived_kk' => $derivedTotals['kk'],
            'derived_jiwa' => $derivedTotals['jiwa'],
            'derived_nilai_kontrak' => $derivedTotals['nilai_kontrak'],
            'manual_sr' => $manualTotals['sr'],
            'manual_kk' => $manualTotals['kk'],
            'manual_jiwa' => $manualTotals['jiwa'],
            'manual_nilai_kontrak' => $manualTotals['nilai_kontrak'],
        ]);
    }

    /**
     * @return array<string, mixed>
     */
    public function syncToUnit(UnitSpam $unit, string $tahun, string $mode = 'all'): array
    {
        $pekerjaan = $this->airMinumQuery($tahun, null, $unit->desa_id, null)
            ->with(['kegiatan', 'output', 'penerima', 'kontrak', 'progress'])
            ->withCount(['penerima', 'foto'])
            ->get();

        $derived = $this->aggregateDerived($pekerjaan);
        $results = [];

        DB::transaction(function () use ($unit, $tahun, $mode, $pekerjaan, $derived, &$results) {
            if (in_array($mode, ['achievement', 'all'], true)) {
                $results['achievement'] = $unit->achievements()->updateOrCreate(
                    ['tahun' => $tahun],
                    [
                        'jumlah_sr' => $derived['sr'],
                        'jumlah_kk' => $derived['kk'],
                        'jumlah_jiwa' => $derived['jiwa'],
                        'jumlah_bjp_kk' => 0,
                        'jumlah_bjp_jiwa' => 0,
                        'catatan' => 'Disinkronkan dari pekerjaan air minum',
                    ]
                );
            }

            if (in_array($mode, ['budget', 'all'], true)) {
                $results['budgets'] = [];

                foreach ($pekerjaan as $item) {
                    $metrics = $this->derivedMetricsForPekerjaan($item);

                    if ($metrics['nilai_kontrak'] <= 0) {
                        continue;
                    }

                    $results['budgets'][] = $unit->budgets()->updateOrCreate(
                        [
                            'tahun' => $tahun,
                            'nama_paket' => $item->nama_paket,
                        ],
                        [
                            'nilai_kontrak' => $metrics['nilai_kontrak'],
                            'sumber_dana' => $item->kegiatan?->sumber_dana,
                        ]
                    );
                }
            }
        });

        return $results;
    }

    private function integrationDesaQuery(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null,
        ?string $search = null
    ): Builder {
        $desaIdsFromUnits = UnitSpam::query()
            ->when($kecamatanId, function (Builder $q) use ($kecamatanId) {
                $q->whereHas('desa', fn (Builder $dq) => $dq->where('kecamatan_id', $kecamatanId));
            })
            ->when($desaId, fn (Builder $q) => $q->where('desa_id', $desaId))
            ->pluck('desa_id');

        $desaIdsFromPekerjaan = $this->airMinumQuery($tahun, $kecamatanId, $desaId, $search)
            ->pluck('desa_id');

        $desaIds = $desaIdsFromUnits
            ->merge($desaIdsFromPekerjaan)
            ->filter()
            ->unique()
            ->values();

        $query = Desa::query()->whereIn('id', $desaIds->isEmpty() ? [-1] : $desaIds);

        if ($kecamatanId) {
            $query->where('kecamatan_id', $kecamatanId);
        }

        if ($desaId) {
            $query->where('id', $desaId);
        }

        if ($search) {
            $term = '%'.$search.'%';
            $unitDesaIds = UnitSpam::query()
                ->where('name', 'like', $term)
                ->pluck('desa_id');

            $query->where(function (Builder $q) use ($term, $unitDesaIds) {
                $q->where('n_desa', 'like', $term)
                    ->orWhereHas('kecamatan', fn (Builder $kq) => $kq->where('n_kec', 'like', $term));

                if ($unitDesaIds->isNotEmpty()) {
                    $q->orWhereIn('id', $unitDesaIds);
                }
            });
        }

        return $query;
    }

    /**
     * @param  array<int, array>  $rows
     * @return array<string, int>
     */
    private function summarizeRows(array $rows): array
    {
        $summary = [
            'total_desa' => count($rows),
            'matched_count' => 0,
            'partial_count' => 0,
            'no_unit_count' => 0,
            'no_pekerjaan_count' => 0,
            'total_pekerjaan' => 0,
            'total_units' => 0,
        ];

        foreach ($rows as $row) {
            match ($row['sync_status']) {
                'matched' => $summary['matched_count']++,
                'partial' => $summary['partial_count']++,
                'no_unit' => $summary['no_unit_count']++,
                'no_pekerjaan' => $summary['no_pekerjaan_count']++,
                default => null,
            };

            $summary['total_pekerjaan'] += $row['pekerjaan_count'];
            $summary['total_units'] += $row['unit_count'];
        }

        return $summary;
    }

    /**
     * @return array<string, mixed>
     */
    private function formatIntegrationPekerjaan(Pekerjaan $pekerjaan): array
    {
        $metrics = $this->derivedMetricsForPekerjaan($pekerjaan);

        return [
            'id' => $pekerjaan->id,
            'nama_paket' => $pekerjaan->nama_paket,
            'pagu' => (float) $pekerjaan->pagu,
            'tahun_anggaran' => (string) ($pekerjaan->kegiatan?->tahun_anggaran ?? ''),
            'sumber_dana' => (string) ($pekerjaan->kegiatan?->sumber_dana ?? ''),
            'progress_total' => $metrics['progress_total'],
            'nilai_kontrak' => $metrics['nilai_kontrak'],
            'sr' => $metrics['sr'],
            'kk' => $metrics['kk'],
            'jiwa' => $metrics['jiwa'],
            'penerima_count' => (int) ($pekerjaan->penerima_count ?? 0),
            'foto_count' => (int) ($pekerjaan->foto_count ?? 0),
        ];
    }

    /**
     * @param  array{sr: int, kk: int, jiwa: int, nilai_kontrak: float}  $derived
     * @param  array{sr: int, kk: int, jiwa: int, nilai_kontrak: float}  $manual
     */
    private function resolveSyncStatus(int $unitCount, int $pekerjaanCount, array $derived, array $manual): string
    {
        if ($unitCount === 0 && $pekerjaanCount > 0) {
            return 'no_unit';
        }

        if ($pekerjaanCount === 0 && $unitCount > 0) {
            return 'no_pekerjaan';
        }

        if ($unitCount === 0 && $pekerjaanCount === 0) {
            return 'partial';
        }

        $metricsMatch = $derived['sr'] === $manual['sr']
            && $derived['kk'] === $manual['kk']
            && $derived['jiwa'] === $manual['jiwa']
            && (int) round($derived['nilai_kontrak']) === (int) round($manual['nilai_kontrak']);

        return $metricsMatch ? 'matched' : 'partial';
    }

    private function calculateProgressTotal(array $content): float
    {
        $items = $content['items'] ?? [];
        $progressTotal = 0.0;

        foreach ($items as $item) {
            $weight = (float) ($item['bobot'] ?? 0);
            $targetVolume = (float) ($item['target_volume'] ?? 0);
            $actual = 0.0;

            foreach (($item['weekly_data'] ?? []) as $weeklyData) {
                if (($weeklyData['realisasi'] ?? null) !== null) {
                    $actual += (float) $weeklyData['realisasi'];
                }
            }

            $progressPercent = $targetVolume > 0 ? ($actual / $targetVolume) * 100 : 0;
            $progressTotal += ($progressPercent * $weight) / 100;
        }

        return round($progressTotal, 2);
    }

    /**
     * @return array<int, array{
     *     desa_id: int,
     *     desa: string,
     *     kecamatan: ?string,
     *     target: int,
     *     unit_count: int,
     *     sr: int,
     *     kk: int,
     *     jiwa: int
     * }>
     */
    public function desaMapStats(?string $tahun = null): array
    {
        $capTahun = $this->manualCapTahun();

        $unitCounts = UnitSpam::query()
            ->select('desa_id', DB::raw('COUNT(*) as unit_count'))
            ->groupBy('desa_id')
            ->pluck('unit_count', 'desa_id');

        $achievementQuery = SpamAchievement::query()
            ->select(
                'tbl_unit_spam.desa_id',
                DB::raw('SUM(tbl_spam_achievements.jumlah_sr) as sr'),
                DB::raw('SUM(tbl_spam_achievements.jumlah_kk) as kk'),
                DB::raw('SUM(tbl_spam_achievements.jumlah_jiwa) as jiwa')
            )
            ->join('tbl_unit_spam', 'tbl_spam_achievements.unit_spam_id', '=', 'tbl_unit_spam.id');

        if ($tahun) {
            $achievementQuery->where('tbl_spam_achievements.tahun', $tahun);
        } else {
            $achievementQuery->where('tbl_spam_achievements.tahun', '<=', $capTahun);
        }

        $achievements = $achievementQuery
            ->groupBy('tbl_unit_spam.desa_id')
            ->get()
            ->keyBy('desa_id');

        return Desa::query()
            ->with('kecamatan:id,n_kec')
            ->orderBy('n_desa')
            ->get(['id', 'n_desa', 'kecamatan_id', 'target'])
            ->map(function (Desa $desa) use ($unitCounts, $achievements) {
                $row = $achievements->get($desa->id);

                return [
                    'desa_id' => $desa->id,
                    'desa' => $desa->n_desa,
                    'kecamatan' => $desa->kecamatan?->n_kec,
                    'target' => (int) $desa->target,
                    'unit_count' => (int) ($unitCounts[$desa->id] ?? 0),
                    'sr' => (int) ($row->sr ?? 0),
                    'kk' => (int) ($row->kk ?? 0),
                    'jiwa' => (int) ($row->jiwa ?? 0),
                ];
            })
            ->values()
            ->all();
    }
}