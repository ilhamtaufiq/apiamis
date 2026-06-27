<?php

namespace App\Services;

use App\Models\Desa;
use App\Models\Output;
use App\Models\Pekerjaan;
use App\Models\SpamAchievement;
use App\Models\SpamBudget;
use App\Models\UnitSpam;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\DB;

class SpamPekerjaanIntegrationService
{
    /** Tahun terakhir acuan master unit SPAM (import awal); tidak ditimpa integrasi. */
    public const BASELINE_CAP_TAHUN = '2025';

    /** Integrasi / akumulasi pekerjaan dimulai dari tahun ini. */
    public const ACCUMULATION_START_TAHUN = '2026';

    public static function isProtectedBaselineTahun(?string $tahun): bool
    {
        if ($tahun === null || $tahun === '' || $tahun === 'unknown') {
            return false;
        }

        return (int) $tahun <= (int) self::BASELINE_CAP_TAHUN;
    }

    public static function isAccumulationTahun(?string $tahun): bool
    {
        if ($tahun === null || $tahun === '' || $tahun === 'unknown') {
            return false;
        }

        return (int) $tahun >= (int) self::ACCUMULATION_START_TAHUN;
    }

    public function baselineCapTahun(): string
    {
        return self::BASELINE_CAP_TAHUN;
    }

    public function accumulationStartTahun(): string
    {
        return self::ACCUMULATION_START_TAHUN;
    }

    public static function applyAccumulationTahunScope(Builder $query): void
    {
        $query->where('tahun_anggaran', '>=', self::ACCUMULATION_START_TAHUN);
    }

    public static function isAirMinumKomponen(string $komponen): bool
    {
        return self::classifyAirMinumKomponen($komponen) !== null;
    }

    public static function classifyAirMinumKomponen(string $komponen): ?string
    {
        $normalized = mb_strtolower(trim($komponen));

        if (str_contains($normalized, 'sambungan') && str_contains($normalized, 'rumah')) {
            return 'sambungan_rumah';
        }

        if (preg_match('/\bsr\b/', $normalized) && ! str_contains($normalized, 'reservoir')) {
            return 'sambungan_rumah';
        }

        if (str_contains($normalized, 'box sr') || str_contains($normalized, 'box sambungan')) {
            return 'sambungan_rumah';
        }

        if (str_contains($normalized, 'pipa') || str_contains($normalized, 'perpipaan') || str_contains($normalized, 'jaringan')) {
            return 'pipa_jaringan';
        }

        if (str_contains($normalized, 'reservoir') || str_contains($normalized, 'tandon') || str_contains($normalized, 'penampung')) {
            return 'reservoir';
        }

        if (str_contains($normalized, 'sumur')) {
            return 'bjp';
        }

        if (
            str_contains($normalized, 'mata air')
            || str_contains($normalized, 'intake')
            || str_contains($normalized, 'sumber air')
            || str_contains($normalized, 'pompa')
        ) {
            return 'sumber_air';
        }

        return null;
    }

    public static function isBjpKomponen(string $komponen): bool
    {
        return self::classifyAirMinumKomponen($komponen) === 'bjp';
    }

    public static function resolveCapaianMetric(?string $override, ?Output $output, Pekerjaan $pekerjaan): string
    {
        if (in_array($override, ['jp', 'bjp'], true)) {
            return $override;
        }

        if ($output && self::isBjpKomponen((string) $output->komponen)) {
            return 'bjp';
        }

        $pekerjaan->loadMissing('output');
        foreach ($pekerjaan->output as $item) {
            if (self::isBjpKomponen((string) $item->komponen)) {
                return 'bjp';
            }
        }

        return 'jp';
    }

    public static function applyAirMinumOutputScope(Builder $query): void
    {
        $query->where(function (Builder $inner) {
            $inner->whereRaw('LOWER(komponen) LIKE ?', ['%sambungan%rumah%'])
                ->orWhereRaw('LOWER(komponen) REGEXP ?', ['(^|[^a-z])sr([^a-z]|$)'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%box sr%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%pipa%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%perpipaan%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%jaringan%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%reservoir%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%tandon%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%penampung%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%sumur%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%mata air%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%intake%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%sumber air%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%pompa%']);
        });
    }

    public static function applyOutputTypeSqlFilter(Builder $query, string $outputType): void
    {
        match ($outputType) {
            'sambungan_rumah' => $query->where(function (Builder $inner) {
                $inner->whereRaw('LOWER(komponen) LIKE ?', ['%sambungan%rumah%'])
                    ->orWhereRaw('LOWER(komponen) REGEXP ?', ['(^|[^a-z])sr([^a-z]|$)'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%box sr%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%box sambungan%']);
            }),
            'pipa_jaringan' => $query->where(function (Builder $inner) {
                $inner->whereRaw('LOWER(komponen) LIKE ?', ['%pipa%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%perpipaan%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%jaringan%']);
            }),
            'reservoir' => $query->where(function (Builder $inner) {
                $inner->whereRaw('LOWER(komponen) LIKE ?', ['%reservoir%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%tandon%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%penampung%']);
            }),
            'bjp' => $query->whereRaw('LOWER(komponen) LIKE ?', ['%sumur%']),
            'sumber_air' => $query->where(function (Builder $inner) {
                $inner->whereRaw('LOWER(komponen) LIKE ?', ['%mata air%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%intake%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%sumber air%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%pompa%']);
            }),
            default => $query->whereRaw('1 = 0'),
        };
    }
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
        ?string $search = null,
        ?string $outputType = null,
        ?string $komponen = null,
        ?int $unitSpamId = null,
        ?bool $accumulationOnly = null,
    ): Builder {
        $forwardOnly = $accumulationOnly === true || ($accumulationOnly === null && $tahun === null);

        $query = Pekerjaan::query()
            ->byUserRole()
            ->whereHas('kegiatan', function (Builder $q) use ($tahun, $forwardOnly) {
                $q->where('sub_bidang', 'Air Minum');

                if ($tahun) {
                    $q->where('tahun_anggaran', $tahun);
                } elseif ($forwardOnly) {
                    self::applyAccumulationTahunScope($q);
                }
            })
            ->whereHas('output', function (Builder $q) use ($outputType, $komponen) {
                if ($komponen) {
                    $q->where('komponen', $komponen);

                    return;
                }

                self::applyAirMinumOutputScope($q);

                if ($outputType) {
                    $q->where(function (Builder $filtered) use ($outputType) {
                        self::applyOutputTypeSqlFilter($filtered, $outputType);
                    });
                }
            });

        if ($kecamatanId) {
            $query->where('kecamatan_id', $kecamatanId);
        }

        if ($desaId) {
            $query->where('desa_id', $desaId);
        }

        if ($unitSpamId) {
            $unit = UnitSpam::query()->find($unitSpamId);
            if ($unit?->desa_id) {
                $query->where('desa_id', $unit->desa_id);
            }
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
     * @return array<int, array{komponen: string, output_type: ?string, is_integrasi: bool, pekerjaan_count: int, label: string}>
     */
    public function listIntegrationOutputOptions(?string $tahun = null, ?int $kecamatanId = null): array
    {
        $query = Output::query()
            ->select('komponen')
            ->selectRaw('COUNT(DISTINCT pekerjaan_id) as pekerjaan_count')
            ->whereHas('pekerjaan', function (Builder $q) use ($tahun, $kecamatanId) {
                $q->byUserRole()
                    ->whereHas('kegiatan', function (Builder $kq) use ($tahun) {
                        $kq->where('sub_bidang', 'Air Minum');

                        if ($tahun) {
                            $kq->where('tahun_anggaran', $tahun);
                        } else {
                            self::applyAccumulationTahunScope($kq);
                        }
                    });

                if ($kecamatanId) {
                    $q->where('kecamatan_id', $kecamatanId);
                }
            })
            ->groupBy('komponen')
            ->orderBy('komponen');

        return $query->get()
            ->map(function ($row) {
                $komponen = (string) $row->komponen;
                $outputType = self::classifyAirMinumKomponen($komponen);

                return [
                    'komponen' => $komponen,
                    'output_type' => $outputType,
                    'is_integrasi' => self::isAirMinumKomponen($komponen),
                    'pekerjaan_count' => (int) $row->pekerjaan_count,
                    'label' => $komponen,
                ];
            })
            ->values()
            ->all();
    }

    /**
     * @return array{sr: int, kk: int, jiwa: int, nilai_kontrak: float, progress_total: float}
     */
    /**
     * @return array<int, array<string, mixed>>
     */
    public function airMinumOutputsForPekerjaan(Pekerjaan $pekerjaan, bool $includeUnclassified = false): array
    {
        $pekerjaan->loadMissing('output');

        return $pekerjaan->output
            ->filter(function (Output $output) use ($includeUnclassified) {
                if ($includeUnclassified) {
                    return true;
                }

                return self::isAirMinumKomponen((string) $output->komponen);
            })
            ->map(function (Output $output) {
                $outputType = self::classifyAirMinumKomponen((string) $output->komponen);

                return [
                    'id' => $output->id,
                    'komponen' => $output->komponen,
                    'satuan' => $output->satuan,
                    'volume' => (float) $output->volume,
                    'output_type' => $outputType,
                    'suggested_capaian_metric' => $outputType === 'bjp' ? 'bjp' : ($outputType === null ? 'bjp' : 'jp'),
                ];
            })
            ->values()
            ->all();
    }

    public function derivedMetricsForPekerjaan(Pekerjaan $pekerjaan, ?string $capaianMetric = null): array
    {
        $pekerjaan->loadMissing(['output', 'penerima', 'kontrak', 'progress', 'kegiatan']);

        $selectedOutput = null;
        if ($pekerjaan->pivot?->output_id) {
            $selectedOutput = $pekerjaan->output->firstWhere('id', (int) $pekerjaan->pivot->output_id);
        }

        $resolvedMetric = self::resolveCapaianMetric(
            $capaianMetric ?? $pekerjaan->pivot?->capaian_metric,
            $selectedOutput,
            $pekerjaan,
        );

        $pembiayaan = $this->resolvePembiayaanFromPekerjaan($pekerjaan);
        $progressTotal = $this->calculateProgressTotal($pekerjaan->progress?->content ?? []);
        $penerimaKk = (int) ($pekerjaan->penerima_count ?? $pekerjaan->penerima->count());
        $jiwaSum = (int) $pekerjaan->penerima->sum('jumlah_jiwa');

        if ($resolvedMetric === 'bjp') {
            $bjpKk = $penerimaKk;
            if ($bjpKk === 0) {
                foreach ($this->airMinumOutputsForPekerjaan($pekerjaan, true) as $output) {
                    if ($resolvedMetric === 'bjp') {
                        $bjpKk += (int) round((float) $output['volume']);
                    }
                }
            }

            $bjpJiwa = $jiwaSum > 0 ? $jiwaSum : ($bjpKk * 5);

            return [
                'sr' => 0,
                'kk' => 0,
                'jiwa' => 0,
                'bjp_kk' => $bjpKk,
                'bjp_jiwa' => $bjpJiwa,
                'capaian_metric' => 'bjp',
                'nilai_kontrak' => $pembiayaan,
                'pembiayaan_suggested' => $pembiayaan,
                'progress_total' => $progressTotal,
            ];
        }

        $sr = 0;
        foreach ($this->airMinumOutputsForPekerjaan($pekerjaan) as $output) {
            if ($output['output_type'] === 'sambungan_rumah') {
                $sr += (int) round((float) $output['volume']);
            }
        }

        $kk = $penerimaKk;
        if ($kk === 0 && $sr > 0) {
            $kk = $sr;
        }

        $jiwa = $jiwaSum > 0 ? $jiwaSum : ($kk * 5);

        return [
            'sr' => $sr,
            'kk' => $kk,
            'jiwa' => $jiwa,
            'bjp_kk' => 0,
            'bjp_jiwa' => 0,
            'capaian_metric' => 'jp',
            'nilai_kontrak' => $pembiayaan,
            'pembiayaan_suggested' => $pembiayaan,
            'progress_total' => $progressTotal,
        ];
    }

    public function resolvePembiayaanFromPekerjaan(Pekerjaan $pekerjaan): float
    {
        $pekerjaan->loadMissing('kontrak');

        $nilaiKontrak = (float) $pekerjaan->kontrak->sum('nilai_kontrak');
        if ($nilaiKontrak > 0) {
            return $nilaiKontrak;
        }

        return (float) ($pekerjaan->pagu ?? 0);
    }

    /**
     * @param  Collection<int, Pekerjaan>|array<int, Pekerjaan>  $pekerjaanCollection
     * @return array{sr: int, kk: int, jiwa: int, bjp_kk: int, bjp_jiwa: int, nilai_kontrak: float, progress_avg: float}
     */
    public function aggregateDerived(Collection|array $pekerjaanCollection): array
    {
        $collection = $pekerjaanCollection instanceof Collection
            ? $pekerjaanCollection
            : collect($pekerjaanCollection);

        $sr = 0;
        $kk = 0;
        $jiwa = 0;
        $bjpKk = 0;
        $bjpJiwa = 0;
        $nilaiKontrak = 0.0;
        $progressValues = [];

        foreach ($collection as $pekerjaan) {
            $metrics = $this->derivedMetricsForPekerjaan($pekerjaan);
            $sr += $metrics['sr'];
            $kk += $metrics['kk'];
            $jiwa += $metrics['jiwa'];
            $bjpKk += $metrics['bjp_kk'] ?? 0;
            $bjpJiwa += $metrics['bjp_jiwa'] ?? 0;
            $nilaiKontrak += $metrics['nilai_kontrak'];
            $progressValues[] = $metrics['progress_total'];
        }

        return [
            'sr' => $sr,
            'kk' => $kk,
            'jiwa' => $jiwa,
            'bjp_kk' => $bjpKk,
            'bjp_jiwa' => $bjpJiwa,
            'nilai_kontrak' => $nilaiKontrak,
            'progress_avg' => count($progressValues) > 0
                ? round(array_sum($progressValues) / count($progressValues), 1)
                : 0.0,
        ];
    }

    private function applyAchievementBudgetTahunScope(
        Builder $achievementQuery,
        Builder $budgetQuery,
        ?string $tahun,
        ?int $minTahun,
        ?int $maxTahun,
    ): void {
        if ($tahun) {
            $achievementQuery->where('tahun', $tahun);
            $budgetQuery->where('tahun', $tahun);
        } elseif ($minTahun !== null) {
            // Cakupan integrasi ke depan: batas bawah saja, jangan gabung dengan cap acuan baseline.
        } elseif ($maxTahun !== null) {
            $achievementQuery->where('tahun', '<=', (string) $maxTahun);
            $budgetQuery->where('tahun', '<=', (string) $maxTahun);
        } else {
            $capTahun = $this->manualCapTahun();
            $achievementQuery->where('tahun', '<=', $capTahun);
            $budgetQuery->where('tahun', '<=', $capTahun);
        }

        if ($minTahun !== null) {
            $achievementQuery->where('tahun', '>=', (string) $minTahun);
            $budgetQuery->where('tahun', '>=', (string) $minTahun);
        }

        if ($maxTahun !== null && $minTahun !== null) {
            $achievementQuery->where('tahun', '<=', (string) $maxTahun);
            $budgetQuery->where('tahun', '<=', (string) $maxTahun);
        }
    }

    /**
     * @return array{sr: int, kk: int, jiwa: int, nilai_kontrak: float}
     */
    public function aggregateManualForDesa(
        int $desaId,
        ?string $tahun = null,
        ?Collection $units = null,
        ?int $minTahun = null,
        ?int $maxTahun = null,
    ): array {
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

        $this->applyAchievementBudgetTahunScope(
            $achievementQuery,
            $budgetQuery,
            $tahun,
            $minTahun,
            $maxTahun,
        );

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
    public function aggregateManualGlobal(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $minTahun = null,
        ?int $maxTahun = null,
    ): array {
        $achievementQuery = SpamAchievement::query();
        $budgetQuery = SpamBudget::query();

        $this->applyAchievementBudgetTahunScope(
            $achievementQuery,
            $budgetQuery,
            $tahun,
            $minTahun,
            $maxTahun,
        );

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

    public function buildDesaIntegrationRow(
        Desa $desa,
        ?string $tahun = null,
        ?string $outputType = null,
        ?string $komponen = null,
    ): array {
        $desa->loadMissing('kecamatan');

        $units = UnitSpam::query()
            ->with(['pengelola', 'pekerjaan'])
            ->where('desa_id', $desa->id)
            ->get();

        $pekerjaan = $this->airMinumQuery($tahun, null, $desa->id, null, $outputType, $komponen, null, true)
            ->with(['kegiatan', 'output', 'penerima', 'kontrak', 'progress', 'unitSpam'])
            ->withCount(['penerima', 'foto'])
            ->get();

        $derived = $this->aggregateDerived($pekerjaan);
        $manual = $this->aggregateManualForDesa($desa->id, $tahun, $units);
        $manualIntegrasi = $this->aggregateManualForDesa(
            $desa->id,
            $tahun,
            $units,
            (int) self::ACCUMULATION_START_TAHUN,
        );
        $linkedCount = $pekerjaan->filter(fn (Pekerjaan $p) => $p->unitSpam->isNotEmpty())->count();

        $formattedPekerjaan = $pekerjaan
            ->map(fn (Pekerjaan $item) => $this->formatAirMinumPekerjaan($item))
            ->values()
            ->all();

        $outputTypes = collect($formattedPekerjaan)
            ->flatMap(fn (array $item) => $item['output_types'] ?? [])
            ->filter()
            ->unique()
            ->values()
            ->all();

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
                'linked_pekerjaan_count' => $unit->pekerjaan->count(),
            ])->values()->all(),
            'unit_count' => $units->count(),
            'pekerjaan_count' => $pekerjaan->count(),
            'linked_count' => $linkedCount,
            'pekerjaan' => $formattedPekerjaan,
            'output_types' => $outputTypes,
            'output_type_filter' => $outputType,
            'derived' => $derived,
            'manual' => $manual,
            'manual_integrasi' => $manualIntegrasi,
            'baseline_cap_tahun' => self::BASELINE_CAP_TAHUN,
            'accumulation_start_tahun' => self::ACCUMULATION_START_TAHUN,
            'sync_status' => $this->resolveSyncStatus($units->count(), $pekerjaan->count(), $linkedCount),
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
        ?string $outputType = null,
        ?string $komponen = null,
        int $perPage = 15,
        int $page = 1
    ): array {
        $rows = $this->integrationDesaQuery($tahun, $kecamatanId, $desaId, $search, $outputType, $komponen)
            ->with('kecamatan')
            ->orderBy('n_desa')
            ->get()
            ->map(fn (Desa $desa) => $this->buildDesaIntegrationRow($desa, $tahun, $outputType, $komponen));

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
            'summary' => $this->summarizeRows($rows->all()),
        ];
    }

    public function paginateAirMinumPekerjaan(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null,
        ?string $search = null,
        ?string $outputType = null,
        ?int $unitSpamId = null,
        ?bool $unlinkedOnly = null,
        int $perPage = 15,
        int $page = 1,
    ): array {
        $query = $this->airMinumQuery($tahun, $kecamatanId, $desaId, $search, $outputType, null, $unitSpamId, true)
            ->with(['kegiatan', 'desa.kecamatan', 'kecamatan', 'output', 'penerima', 'kontrak', 'unitSpam'])
            ->withCount(['penerima', 'foto'])
            ->orderByDesc('id');

        if ($unlinkedOnly && $unitSpamId) {
            $query->whereDoesntHave('unitSpam', fn (Builder $q) => $q->where('tbl_unit_spam.id', $unitSpamId));
        }

        $paginator = $query->paginate($perPage, ['*'], 'page', $page);

        return [
            'data' => collect($paginator->items())
                ->map(fn (Pekerjaan $item) => $this->formatAirMinumPekerjaan($item, $unitSpamId))
                ->values()
                ->all(),
            'meta' => [
                'current_page' => $paginator->currentPage(),
                'last_page' => $paginator->lastPage(),
                'per_page' => $paginator->perPage(),
                'total' => $paginator->total(),
            ],
        ];
    }

    public function attachPekerjaan(
        UnitSpam $unitSpam,
        int $pekerjaanId,
        ?int $outputId = null,
        ?string $capaianMetric = null,
    ): void {
        $pekerjaan = $this->findPekerjaanForAttach($unitSpam, $pekerjaanId, $capaianMetric);

        $output = null;
        if ($outputId) {
            $output = Output::query()
                ->where('pekerjaan_id', $pekerjaan->id)
                ->where('id', $outputId)
                ->firstOrFail();

            $resolvedMetric = self::resolveCapaianMetric($capaianMetric, $output, $pekerjaan);
            if (
                $resolvedMetric !== 'bjp'
                && ! self::isAirMinumKomponen((string) $output->komponen)
            ) {
                throw new \InvalidArgumentException('Output bukan komponen air minum yang didukung.');
            }
        }

        $resolvedMetric = self::resolveCapaianMetric($capaianMetric, $output, $pekerjaan);

        $unitSpam->pekerjaan()->syncWithoutDetaching([
            $pekerjaanId => [
                'output_id' => $outputId,
                'capaian_metric' => $resolvedMetric,
            ],
        ]);

        $this->syncUnitAccumulationFromLinks($unitSpam);
    }

    private function findPekerjaanForAttach(UnitSpam $unitSpam, int $pekerjaanId, ?string $capaianMetric): Pekerjaan
    {
        if ($capaianMetric === 'bjp') {
            $query = Pekerjaan::query()
                ->byUserRole()
                ->where('id', $pekerjaanId)
                ->whereHas('kegiatan', fn (Builder $q) => $q->where('sub_bidang', 'Air Minum'));

            if ($unitSpam->desa_id) {
                $query->where('desa_id', $unitSpam->desa_id);
            }

            return $query->firstOrFail();
        }

        return $this->airMinumQuery(null, null, null, null, null, null, $unitSpam->id)
            ->where('id', $pekerjaanId)
            ->firstOrFail();
    }

    public function detachPekerjaan(UnitSpam $unitSpam, int $pekerjaanId): void
    {
        $unitSpam->pekerjaan()->detach($pekerjaanId);

        $this->syncUnitAccumulationFromLinks($unitSpam);
    }

    /**
     * @return array<string, mixed>
     */
    public function syncUnitAccumulationFromLinks(UnitSpam $unitSpam): array
    {
        $unitSpam->load(['pekerjaan.kegiatan', 'pekerjaan.kontrak', 'pekerjaan.output', 'pekerjaan.penerima', 'pekerjaan.progress']);

        $results = ['achievements' => [], 'budgets' => []];

        DB::transaction(function () use ($unitSpam, &$results) {
            $linkedPekerjaan = $unitSpam->pekerjaan;

            $byTahun = $linkedPekerjaan->groupBy(
                fn (Pekerjaan $p) => (string) ($p->kegiatan?->tahun_anggaran ?? 'unknown')
            );

            $activeIntegrasiTahun = [];
            foreach ($byTahun as $tahun => $group) {
                if ($tahun === 'unknown' || ! self::isAccumulationTahun($tahun)) {
                    continue;
                }

                $activeIntegrasiTahun[] = $tahun;
                $derived = $this->aggregateDerived($group);

                $results['achievements'][] = $unitSpam->achievements()->updateOrCreate(
                    ['tahun' => $tahun],
                    [
                        'jumlah_sr' => $derived['sr'],
                        'jumlah_kk' => $derived['kk'],
                        'jumlah_jiwa' => $derived['jiwa'],
                        'jumlah_bjp_kk' => $derived['bjp_kk'] ?? 0,
                        'jumlah_bjp_jiwa' => $derived['bjp_jiwa'] ?? 0,
                        'catatan' => 'Akumulasi dari paket pekerjaan tertaut',
                    ]
                );
            }

            $integrasiAchievementQuery = $unitSpam->achievements()
                ->where('catatan', 'Akumulasi dari paket pekerjaan tertaut')
                ->where('tahun', '>=', self::ACCUMULATION_START_TAHUN);

            if ($activeIntegrasiTahun !== []) {
                $integrasiAchievementQuery
                    ->whereNotIn('tahun', $activeIntegrasiTahun)
                    ->delete();
            } else {
                $integrasiAchievementQuery->delete();
            }

            $linkedIds = $linkedPekerjaan->pluck('id');

            foreach ($linkedPekerjaan as $item) {
                $metrics = $this->derivedMetricsForPekerjaan($item);
                $tahun = (string) ($item->kegiatan?->tahun_anggaran ?? '');

                if ($tahun === '' || ! self::isAccumulationTahun($tahun) || $metrics['nilai_kontrak'] <= 0) {
                    continue;
                }

                $results['budgets'][] = $unitSpam->budgets()->updateOrCreate(
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

            $desaPekerjaan = $this->airMinumQuery(null, null, $unitSpam->desa_id, null, null, null, null, true)
                ->with('kegiatan')
                ->get();

            foreach ($desaPekerjaan as $candidate) {
                if ($linkedIds->contains($candidate->id)) {
                    continue;
                }

                $tahun = (string) ($candidate->kegiatan?->tahun_anggaran ?? '');
                if ($tahun === '' || ! self::isAccumulationTahun($tahun)) {
                    continue;
                }

                $unitSpam->budgets()
                    ->where('tahun', $tahun)
                    ->where('nama_paket', $candidate->nama_paket)
                    ->delete();
            }
        });

        return $results;
    }

    public function linkedPekerjaanQuery(?string $tahun = null, ?int $kecamatanId = null): Builder
    {
        $query = Pekerjaan::query()
            ->byUserRole()
            ->whereHas('unitSpam')
            ->whereHas('kegiatan', function (Builder $q) use ($tahun) {
                $q->where('sub_bidang', 'Air Minum');

                if ($tahun) {
                    $q->where('tahun_anggaran', $tahun);
                } else {
                    self::applyAccumulationTahunScope($q);
                }
            });

        if ($kecamatanId) {
            $query->where('kecamatan_id', $kecamatanId);
        }

        return $query;
    }

    public function countLinkedUnits(?int $kecamatanId = null): int
    {
        $query = UnitSpam::query()->whereHas('pekerjaan');

        if ($kecamatanId) {
            $query->whereHas('desa', fn (Builder $q) => $q->where('kecamatan_id', $kecamatanId));
        }

        return (int) $query->count();
    }

    /**
     * @return array{
     *     linked_pekerjaan_count: int,
     *     linked_units_count: int,
     *     paket_belum_tertaut: int,
     *     linked_sr: int,
     *     linked_kk: int,
     *     linked_jiwa: int,
     *     linked_nilai_kontrak: float,
     *     capaian_sr: int,
     *     capaian_kk: int,
     *     capaian_jiwa: int,
     *     capaian_nilai_kontrak: float,
     *     potensi_sr: int,
     *     potensi_kk: int,
     *     potensi_jiwa: int,
     *     potensi_nilai_kontrak: float,
     *     selisih_sr: int,
     *     selisih_kk: int,
     *     selisih_jiwa: int,
     *     selisih_nilai_kontrak: float,
     * }
     */
    public function buildStatsEnrichment(?string $tahun = null, ?int $kecamatanId = null): array
    {
        $capaianIntegrasi = $this->aggregateManualGlobal(
            $tahun,
            $kecamatanId,
            (int) self::ACCUMULATION_START_TAHUN,
        );
        $capaianBaseline = $this->aggregateManualGlobal(
            $tahun,
            $kecamatanId,
            null,
            (int) self::BASELINE_CAP_TAHUN,
        );

        $capaian = $tahun
            ? $this->aggregateManualGlobal($tahun, $kecamatanId)
            : [
                'sr' => $capaianBaseline['sr'] + $capaianIntegrasi['sr'],
                'kk' => $capaianBaseline['kk'] + $capaianIntegrasi['kk'],
                'jiwa' => $capaianBaseline['jiwa'] + $capaianIntegrasi['jiwa'],
                'nilai_kontrak' => $capaianBaseline['nilai_kontrak'] + $capaianIntegrasi['nilai_kontrak'],
            ];

        $potensiPekerjaan = $this->airMinumQuery($tahun, $kecamatanId, null, null, null, null, null, true)
            ->with(['output', 'penerima', 'kontrak', 'progress'])
            ->withCount('penerima')
            ->get();

        $potensi = $this->aggregateDerived($potensiPekerjaan);

        $linkedPekerjaan = $this->linkedPekerjaanQuery($tahun, $kecamatanId)
            ->with(['output', 'penerima', 'kontrak', 'progress'])
            ->withCount('penerima')
            ->get();

        $linked = $this->aggregateDerived($linkedPekerjaan);

        $linkedCount = $linkedPekerjaan->count();
        $potensiCount = $potensiPekerjaan->count();

        return [
            'linked_pekerjaan_count' => $linkedCount,
            'linked_units_count' => $this->countLinkedUnits($kecamatanId),
            'paket_belum_tertaut' => max(0, $potensiCount - $linkedCount),
            'linked_sr' => $linked['sr'],
            'linked_kk' => $linked['kk'],
            'linked_jiwa' => $linked['jiwa'],
            'linked_nilai_kontrak' => $linked['nilai_kontrak'],
            'baseline_cap_tahun' => self::BASELINE_CAP_TAHUN,
            'accumulation_start_tahun' => self::ACCUMULATION_START_TAHUN,
            'capaian_sr' => $capaian['sr'],
            'capaian_kk' => $capaian['kk'],
            'capaian_jiwa' => $capaian['jiwa'],
            'capaian_nilai_kontrak' => $capaian['nilai_kontrak'],
            'capaian_baseline_sr' => $capaianBaseline['sr'],
            'capaian_baseline_kk' => $capaianBaseline['kk'],
            'capaian_baseline_jiwa' => $capaianBaseline['jiwa'],
            'capaian_baseline_nilai_kontrak' => $capaianBaseline['nilai_kontrak'],
            'capaian_integrasi_sr' => $capaianIntegrasi['sr'],
            'capaian_integrasi_kk' => $capaianIntegrasi['kk'],
            'capaian_integrasi_jiwa' => $capaianIntegrasi['jiwa'],
            'capaian_integrasi_nilai_kontrak' => $capaianIntegrasi['nilai_kontrak'],
            'potensi_sr' => $potensi['sr'],
            'potensi_kk' => $potensi['kk'],
            'potensi_jiwa' => $potensi['jiwa'],
            'potensi_nilai_kontrak' => $potensi['nilai_kontrak'],
            'selisih_sr' => $potensi['sr'] - $capaianIntegrasi['sr'],
            'selisih_kk' => $potensi['kk'] - $capaianIntegrasi['kk'],
            'selisih_jiwa' => $potensi['jiwa'] - $capaianIntegrasi['jiwa'],
            'selisih_nilai_kontrak' => $potensi['nilai_kontrak'] - $capaianIntegrasi['nilai_kontrak'],
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
            'pekerjaan_air_minum_count' => (int) $this->airMinumQuery($tahun, $kecamatanId, $desaId, null, null, null, null, true)->count(),
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
    /**
     * @deprecated Gunakan attachPekerjaan + syncUnitAccumulationFromLinks
     */
    public function syncToUnit(UnitSpam $unit, string $tahun, string $mode = 'all'): array
    {
        $pekerjaan = $this->airMinumQuery($tahun, null, $unit->desa_id, null, null, null, null, true)
            ->with(['kegiatan', 'output', 'penerima', 'kontrak', 'progress'])
            ->withCount(['penerima', 'foto'])
            ->get();

        foreach ($pekerjaan as $item) {
            $tahunAnggaran = (string) ($item->kegiatan?->tahun_anggaran ?? '');
            if (! self::isAccumulationTahun($tahunAnggaran)) {
                continue;
            }

            $unit->pekerjaan()->syncWithoutDetaching([$item->id => ['output_id' => null]]);
        }

        return $this->syncUnitAccumulationFromLinks($unit->fresh());
    }

    private function integrationDesaQuery(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null,
        ?string $search = null,
        ?string $outputType = null,
        ?string $komponen = null,
    ): Builder {
        $desaIdsFromUnits = UnitSpam::query()
            ->when($kecamatanId, function (Builder $q) use ($kecamatanId) {
                $q->whereHas('desa', fn (Builder $dq) => $dq->where('kecamatan_id', $kecamatanId));
            })
            ->when($desaId, fn (Builder $q) => $q->where('desa_id', $desaId))
            ->pluck('desa_id');

        $desaIdsFromPekerjaan = $this->airMinumQuery($tahun, $kecamatanId, $desaId, $search, $outputType, $komponen, null, true)
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
            'total_linked' => 0,
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
            $summary['total_linked'] += $row['linked_count'] ?? 0;
        }

        return $summary;
    }

    /**
     * @return array<string, mixed>
     */
    public function formatAirMinumPekerjaan(Pekerjaan $pekerjaan, ?int $linkedUnitId = null): array
    {
        $pekerjaan->loadMissing(['kegiatan', 'desa.kecamatan', 'kecamatan', 'unitSpam']);

        $metrics = $this->derivedMetricsForPekerjaan($pekerjaan);
        $airMinumOutputs = $this->airMinumOutputsForPekerjaan($pekerjaan);

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
            'bjp_kk' => $metrics['bjp_kk'] ?? 0,
            'bjp_jiwa' => $metrics['bjp_jiwa'] ?? 0,
            'capaian_metric' => $metrics['capaian_metric'] ?? 'jp',
            'penerima_count' => (int) ($pekerjaan->penerima_count ?? 0),
            'foto_count' => (int) ($pekerjaan->foto_count ?? 0),
            'air_minum_outputs' => $airMinumOutputs,
            'output_types' => collect($airMinumOutputs)->pluck('output_type')->filter()->unique()->values()->all(),
            'derived' => $metrics,
            'is_linked' => $linkedUnitId
                ? $pekerjaan->unitSpam->contains('id', $linkedUnitId)
                : $pekerjaan->unitSpam->isNotEmpty(),
            'linked_unit_ids' => $pekerjaan->unitSpam->pluck('id')->values()->all(),
        ];
    }

    private function resolveSyncStatus(int $unitCount, int $pekerjaanCount, int $linkedCount): string
    {
        if ($unitCount === 0 && $pekerjaanCount === 0) {
            return 'no_data';
        }

        if ($unitCount === 0) {
            return 'no_unit';
        }

        if ($pekerjaanCount === 0) {
            return 'no_pekerjaan';
        }

        if ($linkedCount > 0) {
            return 'matched';
        }

        return 'partial';
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