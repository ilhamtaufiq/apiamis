<?php

namespace App\Services;

use App\Models\Desa;
use App\Models\SpmSanitasi;
use Illuminate\Contracts\Pagination\LengthAwarePaginator;
use Illuminate\Database\Eloquent\Builder;

class SpmSanitasiCapaianService
{
    public const JIWA_PER_KK = 5;

    public function scopeLabel(?string $tahun = null): string
    {
        return $tahun
            ? "Infrastruktur tahun konstruksi {$tahun}"
            : 'Semua infrastruktur terdata';
    }

    public function summary(?int $kecamatanId = null, ?string $jenis = null, ?string $tahun = null): array
    {
        $desaQuery = $this->desaBaseQuery($kecamatanId);
        $totalPenduduk = (int) $desaQuery->sum('jumlah_penduduk');
        $totalDesa = (int) $desaQuery->count();
        $targetKk = $totalPenduduk > 0 ? (int) round($totalPenduduk / self::JIWA_PER_KK) : 0;

        $pemanfaatQuery = $this->pemanfaatBaseQuery($kecamatanId, $jenis, $tahun);
        $totalPemanfaatKk = (int) $pemanfaatQuery->sum('jumlah_pemanfaat_kk');
        $totalPemanfaatJiwa = $totalPemanfaatKk * self::JIWA_PER_KK;

        $byJenis = $this->infrastrukturBaseQuery($kecamatanId, $jenis, $tahun)
            ->selectRaw('jenis, SUM(jumlah_pemanfaat_kk) as total_kk, COUNT(*) as unit_count')
            ->groupBy('jenis')
            ->get()
            ->keyBy('jenis');

        $desaWithInfrastruktur = (int) $this->infrastrukturBaseQuery($kecamatanId, $jenis, $tahun)
            ->distinct('desa_id')
            ->count('desa_id');

        $coveragePercentage = $totalPenduduk > 0
            ? round(min(100, ($totalPemanfaatJiwa / $totalPenduduk) * 100), 2)
            : 0;

        $coverageKkPercentage = $targetKk > 0
            ? round(min(100, ($totalPemanfaatKk / $targetKk) * 100), 2)
            : 0;

        return [
            'jiwa_per_kk' => self::JIWA_PER_KK,
            'total_desa' => $totalDesa,
            'desa_with_infrastruktur' => $desaWithInfrastruktur,
            'desa_without_infrastruktur' => max(0, $totalDesa - $desaWithInfrastruktur),
            'total_penduduk' => $totalPenduduk,
            'target_kk' => $targetKk,
            'total_pemanfaat_kk' => $totalPemanfaatKk,
            'total_pemanfaat_jiwa' => $totalPemanfaatJiwa,
            'gap_kk' => max(0, $targetKk - $totalPemanfaatKk),
            'gap_jiwa' => max(0, $totalPenduduk - $totalPemanfaatJiwa),
            'coverage_percentage' => $coveragePercentage,
            'coverage_kk_percentage' => $coverageKkPercentage,
            'by_jenis' => $this->formatByJenis($byJenis),
        ];
    }

    public function paginateDesa(
        ?int $kecamatanId = null,
        ?string $jenis = null,
        ?string $search = null,
        int $page = 1,
        int $perPage = 15,
        string $sort = 'coverage_percentage',
        string $direction = 'asc',
    ): LengthAwarePaginator {
        $query = Desa::query()
            ->realWilayah()
            ->with('kecamatan')
            ->when($kecamatanId, fn (Builder $q) => $q->where('kecamatan_id', $kecamatanId))
            ->when($search, function (Builder $q) use ($search) {
                $like = '%'.$search.'%';
                $q->where(function (Builder $inner) use ($like) {
                    $inner->where('n_desa', 'like', $like)
                        ->orWhereHas('kecamatan', fn (Builder $kq) => $kq->where('n_kec', 'like', $like));
                });
            })
            ->withSum(['spmSanitasi as pemanfaat_kk_total' => function (Builder $q) use ($jenis) {
                if ($jenis) {
                    $q->where('jenis', $jenis);
                }
            }], 'jumlah_pemanfaat_kk')
            ->withSum(['spmSanitasi as pemanfaat_kk_spaldt' => fn (Builder $q) => $q->where('jenis', 'spaldt')], 'jumlah_pemanfaat_kk')
            ->withSum(['spmSanitasi as pemanfaat_kk_spalds' => fn (Builder $q) => $q->where('jenis', 'spalds')], 'jumlah_pemanfaat_kk')
            ->withSum(['spmSanitasi as pemanfaat_kk_iplt' => fn (Builder $q) => $q->where('jenis', 'iplt')], 'jumlah_pemanfaat_kk')
            ->withSum(['spmSanitasi as pemanfaat_kk_mck_individu' => fn (Builder $q) => $q->where('jenis', 'mck_individu')], 'jumlah_pemanfaat_kk')
            ->withSum(['spmSanitasi as pemanfaat_kk_mck_komunal' => fn (Builder $q) => $q->where('jenis', 'mck_komunal')], 'jumlah_pemanfaat_kk')
            ->withCount(['spmSanitasi as unit_count' => function (Builder $q) use ($jenis) {
                if ($jenis) {
                    $q->where('jenis', $jenis);
                }
            }]);

        $allowedSort = ['coverage_percentage', 'jumlah_penduduk', 'pemanfaat_kk', 'n_desa'];
        if (!in_array($sort, $allowedSort, true)) {
            $sort = 'coverage_percentage';
        }
        if (!in_array($direction, ['asc', 'desc'], true)) {
            $direction = 'asc';
        }

        $items = $query->get()
            ->map(fn (Desa $desa) => $this->mapDesaCapaian($desa));

        $sortKey = match ($sort) {
            'jumlah_penduduk' => 'desa.jumlah_penduduk',
            'pemanfaat_kk' => 'pemanfaat_kk',
            'n_desa' => 'desa.n_desa',
            default => 'coverage_percentage',
        };

        $sorted = $items->sortBy(
            fn (array $row) => data_get($row, $sortKey),
            SORT_REGULAR,
            $direction === 'desc'
        )->values();

        $total = $sorted->count();
        $pageItems = $sorted->slice(($page - 1) * $perPage, $perPage)->values();

        return new \Illuminate\Pagination\LengthAwarePaginator(
            $pageItems,
            $total,
            $perPage,
            $page,
            ['path' => request()->url(), 'query' => request()->query()]
        );
    }

    public function mapDesaCapaian(Desa $desa): array
    {
        $penduduk = (int) ($desa->jumlah_penduduk ?? 0);
        $targetKk = $penduduk > 0 ? (int) round($penduduk / self::JIWA_PER_KK) : 0;

        $pemanfaatKk = (int) ($desa->pemanfaat_kk_total ?? 0);
        $pemanfaatJiwa = $pemanfaatKk * self::JIWA_PER_KK;

        $coveragePercentage = $penduduk > 0
            ? round(min(100, ($pemanfaatJiwa / $penduduk) * 100), 2)
            : 0;

        $coverageKkPercentage = $targetKk > 0
            ? round(min(100, ($pemanfaatKk / $targetKk) * 100), 2)
            : 0;

        return [
            'desa' => [
                'id' => $desa->id,
                'n_desa' => $desa->n_desa,
                'jumlah_penduduk' => $penduduk,
                'kecamatan' => [
                    'id' => $desa->kecamatan?->id,
                    'n_kec' => $desa->kecamatan?->n_kec,
                ],
            ],
            'unit_count' => (int) ($desa->unit_count ?? 0),
            'target_kk' => $targetKk,
            'pemanfaat_kk' => $pemanfaatKk,
            'pemanfaat_jiwa' => $pemanfaatJiwa,
            'gap_kk' => max(0, $targetKk - $pemanfaatKk),
            'gap_jiwa' => max(0, $penduduk - $pemanfaatJiwa),
            'coverage_percentage' => $coveragePercentage,
            'coverage_kk_percentage' => $coverageKkPercentage,
            'by_jenis' => [
                'spaldt_kk' => (int) ($desa->pemanfaat_kk_spaldt ?? 0),
                'spalds_kk' => (int) ($desa->pemanfaat_kk_spalds ?? 0),
                'iplt_kk' => (int) ($desa->pemanfaat_kk_iplt ?? 0),
                'mck_individu_kk' => (int) ($desa->pemanfaat_kk_mck_individu ?? 0),
                'mck_komunal_kk' => (int) ($desa->pemanfaat_kk_mck_komunal ?? 0),
            ],
        ];
    }

    private function formatByJenis($byJenis): array
    {
        $jenisList = ['spaldt', 'spalds', 'iplt', 'mck_individu', 'mck_komunal'];
        $result = [];

        foreach ($jenisList as $jenis) {
            $result[$jenis] = [
                'unit_count' => (int) ($byJenis[$jenis]->unit_count ?? 0),
                'pemanfaat_kk' => (int) ($byJenis[$jenis]->total_kk ?? 0),
                'pemanfaat_jiwa' => (int) ($byJenis[$jenis]->total_kk ?? 0) * self::JIWA_PER_KK,
            ];
        }

        return $result;
    }

    private function desaBaseQuery(?int $kecamatanId): Builder
    {
        return Desa::query()
            ->realWilayah()
            ->when($kecamatanId, fn (Builder $q) => $q->where('kecamatan_id', $kecamatanId));
    }

    private function infrastrukturBaseQuery(?int $kecamatanId, ?string $jenis, ?string $tahun = null): Builder
    {
        return SpmSanitasi::query()
            ->whereHas('desa', fn (Builder $dq) => $dq->realWilayah()
                ->when($kecamatanId, fn (Builder $inner) => $inner->where('kecamatan_id', $kecamatanId)))
            ->when($jenis, fn (Builder $q) => $q->where('jenis', $jenis))
            ->when($tahun, fn (Builder $q) => $q->where('tahun_konstruksi', (int) $tahun))
            ->whereNotNull('desa_id');
    }

    private function pemanfaatBaseQuery(?int $kecamatanId, ?string $jenis, ?string $tahun = null): Builder
    {
        return $this->infrastrukturBaseQuery($kecamatanId, $jenis, $tahun);
    }

    private function applyRelationTahunScope(Builder $query, ?string $jenis, ?string $tahun): void
    {
        if ($jenis) {
            $query->where('jenis', $jenis);
        }
        if ($tahun) {
            $query->where('tahun_konstruksi', (int) $tahun);
        }
    }

    /**
     * @return array<int, array{
     *     desa_id: int,
     *     desa: string,
     *     kecamatan: string|null,
     *     jumlah_penduduk: int,
     *     target_kk: int,
     *     unit_count: int,
     *     pemanfaat_kk: int,
     *     pemanfaat_jiwa: int
     * }>
     */
    public function mapStats(?string $jenis = null, ?string $tahun = null): array
    {
        return Desa::query()
            ->realWilayah()
            ->with('kecamatan:id,n_kec')
            ->withSum(['spmSanitasi as pemanfaat_kk_total' => function (Builder $q) use ($jenis, $tahun) {
                $this->applyRelationTahunScope($q, $jenis, $tahun);
            }], 'jumlah_pemanfaat_kk')
            ->withCount(['spmSanitasi as unit_count' => function (Builder $q) use ($jenis, $tahun) {
                $this->applyRelationTahunScope($q, $jenis, $tahun);
            }])
            ->orderBy('n_desa')
            ->get(['id', 'n_desa', 'kecamatan_id', 'jumlah_penduduk'])
            ->map(function (Desa $desa) {
                $penduduk = (int) ($desa->jumlah_penduduk ?? 0);
                $targetKk = $penduduk > 0 ? (int) round($penduduk / self::JIWA_PER_KK) : 0;
                $pemanfaatKk = (int) ($desa->pemanfaat_kk_total ?? 0);

                return [
                    'desa_id' => $desa->id,
                    'desa' => $desa->n_desa,
                    'kecamatan' => $desa->kecamatan?->n_kec,
                    'jumlah_penduduk' => $penduduk,
                    'target_kk' => $targetKk,
                    'unit_count' => (int) ($desa->unit_count ?? 0),
                    'pemanfaat_kk' => $pemanfaatKk,
                    'pemanfaat_jiwa' => $pemanfaatKk * self::JIWA_PER_KK,
                ];
            })
            ->values()
            ->all();
    }
}