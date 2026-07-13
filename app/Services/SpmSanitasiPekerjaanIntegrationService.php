<?php

namespace App\Services;

use App\Models\Desa;
use App\Models\Output;
use App\Models\Pekerjaan;
use App\Models\SpmSanitasi;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Support\Collection;

class SpmSanitasiPekerjaanIntegrationService
{
    public const JIWA_PER_KK = 5;

    /** @deprecated Use isSanitasiKomponen */
    public static function isMckKomponen(string $komponen): bool
    {
        return self::isSanitasiKomponen($komponen);
    }

    public static function isSanitasiKomponen(string $komponen): bool
    {
        return self::classifySanitasiKomponen($komponen) !== null;
    }

    /** @deprecated Use classifySanitasiKomponen */
    public static function classifyMckKomponen(string $komponen): ?string
    {
        return self::classifySanitasiKomponen($komponen);
    }

    public static function classifySanitasiKomponen(string $komponen): ?string
    {
        $normalized = mb_strtolower(trim($komponen));
        // Normalisasi spasi/tanda agar "SPALD-S", "septic_tank" ikut terdeteksi
        $normalized = str_replace(['_', '-', '/', '\\'], ' ', $normalized);
        $normalized = preg_replace('/\s+/', ' ', $normalized) ?? $normalized;

        $isTangkiSeptik = (str_contains($normalized, 'tangki') && str_contains($normalized, 'septik'))
            || str_contains($normalized, 'septic tank')
            || str_contains($normalized, 'septictank')
            || str_contains($normalized, 'spalds')
            || str_contains($normalized, 'spald s')
            || (str_contains($normalized, 'spald') && str_contains($normalized, 's') && ! str_contains($normalized, 'spaldt'));

        if ($isTangkiSeptik || str_contains($normalized, 'spalds')) {
            if (str_contains($normalized, 'komunal')) {
                return 'tangki_septik_komunal';
            }
            if (str_contains($normalized, 'individu') || str_contains($normalized, 'indvidu')) {
                return 'tangki_septik_individu';
            }

            return 'tangki_septik';
        }

        // SPALDT / IPAL / IPLT (pengolahan terpusat)
        if (
            str_contains($normalized, 'ipal')
            || str_contains($normalized, 'iplt')
            || str_contains($normalized, 'spaldt')
            || str_contains($normalized, 'spald t')
        ) {
            return 'ipal';
        }

        if (
            str_contains($normalized, 'mck')
            || str_contains($normalized, 'jamban')
            || str_contains($normalized, 'wc ')
            || str_starts_with($normalized, 'wc')
            || str_contains($normalized, ' toilet')
            || str_starts_with($normalized, 'toilet')
        ) {
            if (str_contains($normalized, 'komunal')) {
                return 'mck_komunal';
            }
            if (str_contains($normalized, 'individu') || str_contains($normalized, 'indvidu')) {
                return 'mck_individu';
            }

            return 'mck';
        }

        return null;
    }

    public static function spmJenisForOutputType(?string $outputType): ?string
    {
        $list = self::spmJenisListForOutputType($outputType);

        return $list[0] ?? null;
    }

    /**
     * @return list<string>
     */
    public static function spmJenisListForOutputType(?string $outputType): array
    {
        return match ($outputType) {
            'mck_individu' => ['mck_individu'],
            'mck_komunal' => ['mck_komunal'],
            'mck' => ['mck_individu', 'mck_komunal'],
            'tangki_septik_individu', 'tangki_septik_komunal', 'tangki_septik' => ['spalds'],
            'ipal' => ['spaldt', 'iplt'],
            default => [],
        };
    }

    /**
     * @return list<string>
     */
    public static function outputTypesForSpmJenis(string $spmJenis): array
    {
        return match ($spmJenis) {
            'spaldt', 'iplt' => ['ipal'],
            'spalds' => ['tangki_septik_individu', 'tangki_septik_komunal', 'tangki_septik'],
            'mck_individu' => ['mck_individu', 'mck'],
            'mck_komunal' => ['mck_komunal', 'mck'],
            default => [],
        };
    }

    public static function outputTypeMatchesSpmJenis(?string $outputType, string $spmJenis): bool
    {
        if ($outputType === null) {
            return false;
        }

        return in_array($outputType, self::outputTypesForSpmJenis($spmJenis), true);
    }

    public static function applySanitasiOutputScope(Builder $query): void
    {
        $query->where(function (Builder $inner) {
            $inner->whereRaw('LOWER(komponen) LIKE ?', ['%mck%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%jamban%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%toilet%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%wc%'])
                ->orWhere(function (Builder $ts) {
                    $ts->where(function (Builder $a) {
                        $a->whereRaw('LOWER(komponen) LIKE ?', ['%tangki%'])
                            ->whereRaw('LOWER(komponen) LIKE ?', ['%septik%']);
                    })
                        ->orWhereRaw('LOWER(komponen) LIKE ?', ['%septic%'])
                        ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spalds%'])
                        ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spald-s%'])
                        ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spald s%']);
                })
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%ipal%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%iplt%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spaldt%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spald-t%'])
                ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spald t%']);
        });
    }

    public static function applyOutputTypeSqlFilter(Builder $query, string $outputType): void
    {
        match ($outputType) {
            'mck_individu' => $query->where(function (Builder $inner) {
                $inner->whereRaw('LOWER(komponen) LIKE ?', ['%mck individu%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%mck indvidu%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%jamban individu%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%toilet individu%']);
            }),
            'mck_komunal' => $query->where(function (Builder $inner) {
                $inner->whereRaw('LOWER(komponen) LIKE ?', ['%mck komunal%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%jamban komunal%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%toilet komunal%']);
            }),
            'mck' => $query->where(function (Builder $inner) {
                $inner->whereRaw('LOWER(komponen) LIKE ?', ['%mck%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%jamban%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%toilet%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%wc%']);
            })
                ->whereRaw('LOWER(komponen) NOT LIKE ?', ['%individu%'])
                ->whereRaw('LOWER(komponen) NOT LIKE ?', ['%indvidu%'])
                ->whereRaw('LOWER(komponen) NOT LIKE ?', ['%komunal%']),
            'tangki_septik_individu' => $query->where(function (Builder $base) {
                $base->where(function (Builder $ts) {
                    $ts->whereRaw('LOWER(komponen) LIKE ?', ['%tangki%'])
                        ->whereRaw('LOWER(komponen) LIKE ?', ['%septik%']);
                })
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%septic%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spalds%']);
            })->where(function (Builder $inner) {
                $inner->whereRaw('LOWER(komponen) LIKE ?', ['%individu%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%indvidu%']);
            }),
            'tangki_septik_komunal' => $query->where(function (Builder $base) {
                $base->where(function (Builder $ts) {
                    $ts->whereRaw('LOWER(komponen) LIKE ?', ['%tangki%'])
                        ->whereRaw('LOWER(komponen) LIKE ?', ['%septik%']);
                })
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%septic%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spalds%']);
            })->whereRaw('LOWER(komponen) LIKE ?', ['%komunal%']),
            'tangki_septik' => $query->where(function (Builder $base) {
                $base->where(function (Builder $ts) {
                    $ts->whereRaw('LOWER(komponen) LIKE ?', ['%tangki%'])
                        ->whereRaw('LOWER(komponen) LIKE ?', ['%septik%']);
                })
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%septic%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spalds%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spald-s%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spald s%']);
            }),
            'ipal' => $query->where(function (Builder $inner) {
                $inner->whereRaw('LOWER(komponen) LIKE ?', ['%ipal%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%iplt%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spaldt%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spald-t%'])
                    ->orWhereRaw('LOWER(komponen) LIKE ?', ['%spald t%']);
            }),
            default => $query->whereRaw('1 = 0'),
        };
    }

    public static function applySpmJenisSqlFilter(Builder $query, string $spmJenis): void
    {
        $types = self::outputTypesForSpmJenis($spmJenis);

        if ($types === []) {
            $query->whereRaw('1 = 0');

            return;
        }

        $query->where(function (Builder $inner) use ($types) {
            foreach ($types as $type) {
                $inner->orWhere(function (Builder $typeQuery) use ($type) {
                    self::applyOutputTypeSqlFilter($typeQuery, $type);
                });
            }
        });
    }

    public function sanitasiPekerjaanQuery(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null,
        ?string $search = null,
        ?string $outputType = null,
        ?int $spmSanitasiId = null,
    ): Builder {
        // Catatan: filter jenis SPM tidak dipaksa di SQL daftar tautan agar paket
        // dengan penamaan output bervariasi tetap muncul. Validasi ketat ada di attachPekerjaan.
        $query = Pekerjaan::query()
            ->byUserRole()
            ->whereHas('output', function (Builder $q) use ($outputType) {
                self::applySanitasiOutputScope($q);

                if ($outputType) {
                    $q->where(function (Builder $filtered) use ($outputType) {
                        self::applyOutputTypeSqlFilter($filtered, $outputType);
                    });
                }
            });

        if ($tahun) {
            $query->whereHas('kegiatan', fn (Builder $q) => $q->where('tahun_anggaran', $tahun));
        }

        if ($kecamatanId) {
            $query->where('kecamatan_id', $kecamatanId);
        }

        if ($desaId) {
            $query->where('desa_id', $desaId);
        }

        if ($spmSanitasiId) {
            $spm = SpmSanitasi::query()->find($spmSanitasiId);
            // Batasi ke desa infrastruktur; filter jenis longgar di daftar (validasi ketat di attach).
            if ($spm?->desa_id) {
                $query->where('desa_id', $spm->desa_id);
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

    /** @deprecated Use sanitasiPekerjaanQuery */
    public function mckPekerjaanQuery(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null,
        ?string $search = null,
        ?string $mckType = null,
        ?int $spmSanitasiId = null,
    ): Builder {
        return $this->sanitasiPekerjaanQuery($tahun, $kecamatanId, $desaId, $search, $mckType, $spmSanitasiId);
    }

    /**
     * @return array<int, array<string, mixed>>
     */
    public function sanitasiOutputsForPekerjaan(Pekerjaan $pekerjaan): array
    {
        $pekerjaan->loadMissing('output');

        return $pekerjaan->output
            ->filter(fn (Output $output) => self::isSanitasiKomponen((string) $output->komponen))
            ->map(function (Output $output) {
                $outputType = self::classifySanitasiKomponen((string) $output->komponen);

                return [
                    'id' => $output->id,
                    'komponen' => $output->komponen,
                    'satuan' => $output->satuan,
                    'volume' => (float) $output->volume,
                    'output_type' => $outputType,
                    'target_jenis' => self::spmJenisForOutputType($outputType),
                    'mck_type' => $outputType,
                ];
            })
            ->values()
            ->all();
    }

    /** @deprecated Use sanitasiOutputsForPekerjaan */
    public function mckOutputsForPekerjaan(Pekerjaan $pekerjaan): array
    {
        return $this->sanitasiOutputsForPekerjaan($pekerjaan);
    }

    /**
     * @return array{unit: int, kk: int, jiwa: int, nilai_kontrak: float, progress_total: float, mck_unit: int}
     */
    public function derivedMetricsForPekerjaan(Pekerjaan $pekerjaan): array
    {
        $pekerjaan->loadMissing(['output', 'penerima', 'kontrak', 'progress']);

        $unit = 0;
        foreach ($this->sanitasiOutputsForPekerjaan($pekerjaan) as $output) {
            $unit += (int) round((float) $output['volume']);
        }

        $kk = (int) ($pekerjaan->penerima_count ?? $pekerjaan->penerima->count());
        if ($kk === 0 && $unit > 0) {
            $kk = $unit;
        }

        $jiwaSum = (int) $pekerjaan->penerima->sum('jumlah_jiwa');
        $jiwa = $jiwaSum > 0 ? $jiwaSum : ($kk * self::JIWA_PER_KK);
        $pembiayaan = $this->resolvePembiayaanFromPekerjaan($pekerjaan);

        return [
            'unit' => $unit,
            'mck_unit' => $unit,
            'kk' => $kk,
            'jiwa' => $jiwa,
            'nilai_kontrak' => $pembiayaan,
            'pembiayaan_suggested' => $pembiayaan,
            'tahun_konstruksi_suggested' => $this->resolveTahunKonstruksiFromPekerjaan($pekerjaan),
            'progress_total' => $this->calculateProgressTotal($pekerjaan->progress?->content ?? []),
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

    public function resolveTahunKonstruksiFromPekerjaan(Pekerjaan $pekerjaan): ?int
    {
        $pekerjaan->loadMissing('kegiatan');

        $tahun = $pekerjaan->kegiatan?->tahun_anggaran;
        if ($tahun === null || $tahun === '') {
            return null;
        }

        $parsed = (int) $tahun;

        return $parsed >= 1900 && $parsed <= 2100 ? $parsed : null;
    }

    public function syncInfrastrukturFromLinkedPekerjaan(SpmSanitasi $spmSanitasi): SpmSanitasi
    {
        $spmSanitasi->load(['pekerjaan.kegiatan', 'pekerjaan.kontrak']);

        if ($spmSanitasi->pekerjaan->isEmpty()) {
            return $spmSanitasi;
        }

        $pembiayaanTotal = 0.0;
        $tahunCandidates = [];

        foreach ($spmSanitasi->pekerjaan as $pekerjaan) {
            $pembiayaanTotal += $this->resolvePembiayaanFromPekerjaan($pekerjaan);

            $tahun = $this->resolveTahunKonstruksiFromPekerjaan($pekerjaan);
            if ($tahun !== null) {
                $tahunCandidates[] = $tahun;
            }
        }

        $updates = [];

        if ($pembiayaanTotal > 0) {
            $updates['pembiayaan_total'] = $pembiayaanTotal;
        }

        if ($spmSanitasi->tahun_konstruksi === null && $tahunCandidates !== []) {
            $updates['tahun_konstruksi'] = min($tahunCandidates);
        }

        if ($updates !== []) {
            $spmSanitasi->update($updates);
        }

        return $spmSanitasi->fresh();
    }

    public function formatSanitasiPekerjaan(Pekerjaan $pekerjaan, ?int $linkedSpmId = null): array
    {
        $pekerjaan->loadMissing(['kegiatan', 'desa.kecamatan', 'spmSanitasi']);

        $metrics = $this->derivedMetricsForPekerjaan($pekerjaan);
        $sanitasiOutputs = $this->sanitasiOutputsForPekerjaan($pekerjaan);

        return [
            'id' => $pekerjaan->id,
            'nama_paket' => $pekerjaan->nama_paket,
            'pagu' => (float) $pekerjaan->pagu,
            'tahun_anggaran' => $pekerjaan->kegiatan?->tahun_anggaran,
            'desa' => [
                'id' => $pekerjaan->desa?->id,
                'n_desa' => $pekerjaan->desa?->n_desa,
            ],
            'kecamatan' => [
                'id' => $pekerjaan->kecamatan?->id,
                'n_kec' => $pekerjaan->kecamatan?->n_kec,
            ],
            'sanitasi_outputs' => $sanitasiOutputs,
            'mck_outputs' => $sanitasiOutputs,
            'output_types' => collect($sanitasiOutputs)->pluck('output_type')->filter()->unique()->values()->all(),
            'mck_types' => collect($sanitasiOutputs)->pluck('output_type')->filter()->unique()->values()->all(),
            'target_jenis_list' => collect($sanitasiOutputs)->pluck('target_jenis')->filter()->unique()->values()->all(),
            'derived' => $metrics,
            'is_linked' => $linkedSpmId
                ? $pekerjaan->spmSanitasi->contains('id', $linkedSpmId)
                : $pekerjaan->spmSanitasi->isNotEmpty(),
            'linked_spm_ids' => $pekerjaan->spmSanitasi->pluck('id')->values()->all(),
        ];
    }

    /** @deprecated Use formatSanitasiPekerjaan */
    public function formatMckPekerjaan(Pekerjaan $pekerjaan, ?int $linkedSpmId = null): array
    {
        return $this->formatSanitasiPekerjaan($pekerjaan, $linkedSpmId);
    }

    public function buildDesaIntegrationRow(Desa $desa, ?string $tahun = null, ?string $outputType = null): array
    {
        $desa->loadMissing('kecamatan');

        $infrastrukturQuery = SpmSanitasi::query()
            ->with(['pekerjaan.kegiatan', 'pekerjaan.output'])
            ->where('desa_id', $desa->id);

        if ($outputType) {
            $jenisList = self::spmJenisListForOutputType($outputType);
            if ($jenisList !== []) {
                $infrastrukturQuery->whereIn('jenis', $jenisList);
            }
        }

        $infrastruktur = $infrastrukturQuery->get();

        $pekerjaan = $this->sanitasiPekerjaanQuery($tahun, null, $desa->id, null, $outputType)
            ->with(['kegiatan', 'output', 'penerima', 'kontrak', 'progress', 'spmSanitasi'])
            ->withCount(['penerima', 'foto'])
            ->get();

        $derived = $this->aggregateDerived($pekerjaan);
        $manual = $this->aggregateManualForDesa($infrastruktur);

        $linkedCount = $pekerjaan->filter(fn (Pekerjaan $p) => $p->spmSanitasi->isNotEmpty())->count();

        $formattedPekerjaan = $pekerjaan
            ->map(fn (Pekerjaan $item) => $this->formatSanitasiPekerjaan($item))
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
                'jumlah_penduduk' => (int) ($desa->jumlah_penduduk ?? 0),
                'kecamatan' => [
                    'id' => $desa->kecamatan->id,
                    'n_kec' => $desa->kecamatan->n_kec,
                ],
            ],
            'infrastruktur' => $infrastruktur->map(fn (SpmSanitasi $item) => [
                'id' => $item->id,
                'jenis' => $item->jenis,
                'nama_infrastruktur' => $item->nama_infrastruktur,
                'jumlah_pemanfaat_kk' => (int) ($item->jumlah_pemanfaat_kk ?? 0),
                'linked_pekerjaan_count' => $item->pekerjaan->count(),
            ])->values()->all(),
            'infrastruktur_count' => $infrastruktur->count(),
            'pekerjaan_count' => $pekerjaan->count(),
            'linked_count' => $linkedCount,
            'pekerjaan' => $formattedPekerjaan,
            'output_types' => $outputTypes,
            'output_type_filter' => $outputType,
            'derived' => $derived,
            'manual' => $manual,
            'sync_status' => $this->resolveSyncStatus(
                $infrastruktur->count(),
                $pekerjaan->count(),
                $linkedCount
            ),
        ];
    }

    /**
     * @return array{data: array<int, array>, meta: array<string, int>, summary: array<string, int|float>}
     */
    public function paginateIntegration(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null,
        ?string $search = null,
        ?string $syncStatus = null,
        ?string $outputType = null,
        int $perPage = 15,
        int $page = 1,
    ): array {
        $query = Desa::query()->with('kecamatan');

        if ($kecamatanId) {
            $query->where('kecamatan_id', $kecamatanId);
        }

        if ($desaId) {
            $query->where('id', $desaId);
        }

        if ($search) {
            $term = '%'.$search.'%';
            $query->where(function (Builder $q) use ($term) {
                $q->where('n_desa', 'like', $term)
                    ->orWhereHas('kecamatan', fn (Builder $kq) => $kq->where('n_kec', 'like', $term));
            });
        }

        $rows = $query->orderBy('n_desa')->get()
            ->map(fn (Desa $desa) => $this->buildDesaIntegrationRow($desa, $tahun, $outputType));

        if ($syncStatus) {
            $rows = $rows->filter(fn (array $row) => $row['sync_status'] === $syncStatus);
        }

        $rows = $rows->filter(function (array $row) {
            return $row['infrastruktur_count'] > 0 || $row['pekerjaan_count'] > 0;
        });

        $total = $rows->count();
        $lastPage = max(1, (int) ceil($total / $perPage));
        $page = min(max(1, $page), $lastPage);
        $offset = ($page - 1) * $perPage;

        $paginatedRows = $rows->slice($offset, $perPage)->values()->all();

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

    public function paginateSanitasiPekerjaan(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null,
        ?string $search = null,
        ?string $outputType = null,
        ?int $spmSanitasiId = null,
        ?bool $unlinkedOnly = null,
        int $perPage = 15,
        int $page = 1,
    ): array {
        $query = $this->sanitasiPekerjaanQuery($tahun, $kecamatanId, $desaId, $search, $outputType, $spmSanitasiId)
            ->with(['kegiatan', 'desa.kecamatan', 'kecamatan', 'output', 'penerima', 'kontrak', 'spmSanitasi'])
            ->withCount(['penerima', 'foto'])
            ->orderByDesc('id');

        if ($unlinkedOnly && $spmSanitasiId) {
            $query->whereDoesntHave('spmSanitasi', fn (Builder $q) => $q->where('tbl_spm_sanitasi.id', $spmSanitasiId));
        }

        $paginator = $query->paginate($perPage, ['*'], 'page', $page);

        return [
            'data' => collect($paginator->items())
                ->map(fn (Pekerjaan $item) => $this->formatSanitasiPekerjaan($item, $spmSanitasiId))
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

    /** @deprecated Use paginateSanitasiPekerjaan */
    public function paginateMckPekerjaan(
        ?string $tahun = null,
        ?int $kecamatanId = null,
        ?int $desaId = null,
        ?string $search = null,
        ?string $mckType = null,
        ?int $spmSanitasiId = null,
        ?bool $unlinkedOnly = null,
        int $perPage = 15,
        int $page = 1,
    ): array {
        return $this->paginateSanitasiPekerjaan(
            $tahun,
            $kecamatanId,
            $desaId,
            $search,
            $mckType,
            $spmSanitasiId,
            $unlinkedOnly,
            $perPage,
            $page,
        );
    }

    public function attachPekerjaan(SpmSanitasi $spmSanitasi, int $pekerjaanId, ?int $outputId = null): void
    {
        $pekerjaan = $this->sanitasiPekerjaanQuery(null, null, null, null, null, $spmSanitasi->id)
            ->where('id', $pekerjaanId)
            ->firstOrFail();

        if ($outputId) {
            $output = Output::query()
                ->where('pekerjaan_id', $pekerjaan->id)
                ->where('id', $outputId)
                ->firstOrFail();

            $outputType = self::classifySanitasiKomponen((string) $output->komponen);

            if (!self::isSanitasiKomponen((string) $output->komponen)) {
                throw new \InvalidArgumentException('Output bukan komponen sanitasi yang didukung.');
            }

            $spmJenis = (string) $spmSanitasi->jenis;
            $jenisOk = self::outputTypeMatchesSpmJenis($outputType, $spmJenis)
                || ($spmJenis === 'iplt' && self::outputTypeMatchesSpmJenis($outputType, 'spaldt'));

            if (! $jenisOk) {
                throw new \InvalidArgumentException(
                    'Output tidak sesuai jenis infrastruktur. Tangki Septik/SPALDS → SPALDS, IPAL/IPLT/SPALDT → SPALDT/IPLT, MCK → MCK.'
                );
            }
        }

        $spmSanitasi->pekerjaan()->syncWithoutDetaching([
            $pekerjaanId => ['output_id' => $outputId],
        ]);

        $this->syncInfrastrukturFromLinkedPekerjaan($spmSanitasi);
    }

    public function detachPekerjaan(SpmSanitasi $spmSanitasi, int $pekerjaanId): void
    {
        $spmSanitasi->pekerjaan()->detach($pekerjaanId);

        $this->syncInfrastrukturFromLinkedPekerjaan($spmSanitasi);
    }

    /**
     * @param  Collection<int, Pekerjaan>|array<int, Pekerjaan>  $pekerjaanCollection
     * @return array{unit: int, mck_unit: int, kk: int, jiwa: int, nilai_kontrak: float, progress_avg: float}
     */
    public function aggregateDerived(Collection|array $pekerjaanCollection): array
    {
        $collection = $pekerjaanCollection instanceof Collection
            ? $pekerjaanCollection
            : collect($pekerjaanCollection);

        $unit = 0;
        $kk = 0;
        $jiwa = 0;
        $nilaiKontrak = 0.0;
        $progressValues = [];

        foreach ($collection as $pekerjaan) {
            $metrics = $this->derivedMetricsForPekerjaan($pekerjaan);
            $unit += $metrics['unit'];
            $kk += $metrics['kk'];
            $jiwa += $metrics['jiwa'];
            $nilaiKontrak += $metrics['nilai_kontrak'];
            $progressValues[] = $metrics['progress_total'];
        }

        return [
            'unit' => $unit,
            'mck_unit' => $unit,
            'kk' => $kk,
            'jiwa' => $jiwa,
            'nilai_kontrak' => $nilaiKontrak,
            'progress_avg' => count($progressValues) > 0
                ? round(array_sum($progressValues) / count($progressValues), 1)
                : 0.0,
        ];
    }

    /**
     * @param  Collection<int, SpmSanitasi>  $infrastruktur
     * @return array{kk: int, jiwa: int, nilai_kontrak: float}
     */
    public function aggregateManualForDesa(Collection $infrastruktur): array
    {
        $kk = (int) $infrastruktur->sum('jumlah_pemanfaat_kk');

        return [
            'kk' => $kk,
            'jiwa' => $kk * self::JIWA_PER_KK,
            'nilai_kontrak' => (float) $infrastruktur->sum('pembiayaan_total'),
        ];
    }

    public function resolveSyncStatus(int $infrastrukturCount, int $pekerjaanCount, int $linkedCount): string
    {
        if ($infrastrukturCount === 0 && $pekerjaanCount === 0) {
            return 'no_data';
        }

        if ($infrastrukturCount === 0) {
            return 'no_infrastruktur';
        }

        if ($pekerjaanCount === 0) {
            return 'no_pekerjaan';
        }

        if ($linkedCount > 0) {
            return 'matched';
        }

        return 'partial';
    }

    /**
     * @param  array<int, array<string, mixed>>  $rows
     * @return array<string, int|float>
     */
    public function summarizeRows(array $rows): array
    {
        $summary = [
            'total_desa' => count($rows),
            'matched_count' => 0,
            'partial_count' => 0,
            'no_infrastruktur_count' => 0,
            'no_pekerjaan_count' => 0,
            'total_infrastruktur' => 0,
            'total_pekerjaan' => 0,
            'total_linked' => 0,
        ];

        foreach ($rows as $row) {
            $summary['total_infrastruktur'] += $row['infrastruktur_count'];
            $summary['total_pekerjaan'] += $row['pekerjaan_count'];
            $summary['total_linked'] += $row['linked_count'];

            match ($row['sync_status']) {
                'matched' => $summary['matched_count']++,
                'partial' => $summary['partial_count']++,
                'no_infrastruktur' => $summary['no_infrastruktur_count']++,
                'no_pekerjaan' => $summary['no_pekerjaan_count']++,
                default => null,
            };
        }

        return $summary;
    }

    private function calculateProgressTotal(mixed $content): float
    {
        if (!is_array($content) || $content === []) {
            return 0.0;
        }

        $values = [];
        foreach ($content as $item) {
            if (is_array($item) && isset($item['progress'])) {
                $values[] = (float) $item['progress'];
            }
        }

        return count($values) > 0 ? round(array_sum($values) / count($values), 1) : 0.0;
    }
}