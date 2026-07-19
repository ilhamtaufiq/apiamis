<?php

namespace App\Http\Controllers;

use App\Http\Resources\PuspenProgressFisikResource;
use App\Models\AppSetting;
use App\Models\Kontrak;
use App\Models\Pekerjaan;
use App\Models\PuspenProgressFisik;
use App\Models\PuspenProgressFisikOutput;
use App\Services\PekerjaanProgressEstimasiSyncService;
use Illuminate\Http\Request;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\Schema;

class PuspenProgressFisikController extends Controller
{
    public function index(Request $request)
    {
        $validated = $request->validate([
            'tahun' => 'nullable|integer|min:2000|max:2100',
            'search' => 'nullable|string|max:100',
            'sub_kegiatan' => 'nullable|string|max:255',
            'page' => 'nullable|integer|min:1',
            'per_page' => 'nullable|integer|min:1|max:1000',
        ]);

        $tahun = (int) ($validated['tahun'] ?? now()->year);
        return $this->progressResponse(
            $request,
            $tahun,
            $validated['search'] ?? null,
            $validated['sub_kegiatan'] ?? null,
            (int) ($validated['per_page'] ?? 15),
        );
    }

    public function publicIndex(Request $request)
    {
        if (AppSetting::getValue('puspen_progress_fisik_public', '0') !== '1') {
            return response()->json(['message' => 'Halaman progress fisik Puspen sedang dikunci'], 403);
        }

        $validated = $request->validate([
            'search' => 'nullable|string|max:100',
            'sub_kegiatan' => 'nullable|string|max:255',
            'page' => 'nullable|integer|min:1',
            'per_page' => 'nullable|integer|min:1|max:1000',
        ]);

        $tahun = (int) (AppSetting::getValue('tahun_anggaran') ?: now()->year);
        return $this->progressResponse(
            $request,
            $tahun,
            $validated['search'] ?? null,
            $validated['sub_kegiatan'] ?? null,
            (int) ($validated['per_page'] ?? 15),
        );
    }

    private function progressResponse(Request $request, int $tahun, ?string $search, ?string $subKegiatan, int $perPage)
    {
        $request->merge(['tahun' => $tahun]);

        $relations = [
            'kegiatan:id,nama_sub_kegiatan,pagu',
            'pekerjaan:id,nama_paket,pagu',
            'pekerjaan.output:id,pekerjaan_id,komponen,satuan,volume',
            'pekerjaans.kegiatan:id,nama_sub_kegiatan',
            'pekerjaans.output:id,pekerjaan_id,komponen,satuan,volume',
            'pekerjaans:id,nama_paket,kegiatan_id,pagu',
            'latestApprovedAddendum',
            'progress_fisik' => fn ($q) => $q->where('tahun_anggaran', $tahun),
        ];

        if (Schema::hasTable('puspen_progress_fisik_output')) {
            $relations['progress_fisik_outputs'] = fn ($q) => $q->where('tahun_anggaran', $tahun);
        }

        $query = Kontrak::query()
            ->with($relations)
            ->where(function ($q) use ($tahun) {
                $q->whereHas('kegiatan', fn ($k) => $k->where('tahun_anggaran', $tahun))
                    ->orWhereHas('pekerjaans.kegiatan', fn ($k) => $k->where('tahun_anggaran', $tahun));
            })
            ->when(
                Schema::hasColumn('tbl_pekerjaan', 'is_konsultan'),
                function ($query) {
                    // Exclude kontrak yang hanya terkait pekerjaan konsultan (tidak ada paket fisik)
                    $query->where(function ($q) {
                        $nonKonsultan = function ($p) {
                            $p->where(function ($inner) {
                                $inner->where('is_konsultan', false)
                                    ->orWhereNull('is_konsultan');
                            });
                        };

                        $q->whereHas('pekerjaan', $nonKonsultan)
                            ->orWhereHas('pekerjaans', $nonKonsultan)
                            ->orWhere(function ($orphan) {
                                // Kontrak tanpa tautan pekerjaan (hanya kegiatan) tetap ditampilkan
                                $orphan->whereDoesntHave('pekerjaan')
                                    ->whereDoesntHave('pekerjaans');
                            });
                    });
                }
            )
            ->orderBy('kode_paket')
            ->orderBy('id');

        if ($search) {
            $query->where(function ($q) use ($search) {
                $q->where('kode_paket', 'like', "%{$search}%")
                    ->orWhereHas('kegiatan', fn ($k) => $k->where('nama_sub_kegiatan', 'like', "%{$search}%"))
                    ->orWhereHas('pekerjaans', fn ($p) => $p->where('nama_paket', 'like', "%{$search}%"))
                    ->orWhereHas('pekerjaans.kegiatan', fn ($k) => $k->where('nama_sub_kegiatan', 'like', "%{$search}%"))
                    ->orWhereHas('pekerjaans.output', fn ($o) => $o->where('komponen', 'like', "%{$search}%"));
            });
        }

        if ($subKegiatan) {
            $query->where(function ($q) use ($subKegiatan) {
                $q->whereHas('kegiatan', fn ($k) => $k->where('nama_sub_kegiatan', $subKegiatan))
                    ->orWhereHas('pekerjaans.kegiatan', fn ($k) => $k->where('nama_sub_kegiatan', $subKegiatan));
            });
        }

        $summary = $this->calculateSummary((clone $query)->get());
        $uncontracted = $this->listUncontractedPekerjaan($tahun, $search, $subKegiatan);

        return PuspenProgressFisikResource::collection($query->paginate($perPage))
            ->additional([
                'summary' => $summary,
                'uncontracted_pekerjaan' => $uncontracted,
            ]);
    }

    /**
     * Paket pekerjaan tahun ini yang belum terhubung ke kontrak (pivot atau id_pekerjaan).
     *
     * @return list<array<string, mixed>>
     */
    private function listUncontractedPekerjaan(int $tahun, ?string $search, ?string $subKegiatan): array
    {
        $query = Pekerjaan::query()
            ->with([
                'kegiatan:id,nama_sub_kegiatan,tahun_anggaran',
                'kecamatan:id,n_kec',
                'desa:id,n_desa',
            ])
            ->whereHas('kegiatan', fn ($k) => $k->where('tahun_anggaran', $tahun))
            // Paket dibatalkan tidak dihitung "belum berkontrak" di progress fisik
            ->when(
                Schema::hasColumn('tbl_pekerjaan', 'status'),
                fn ($q) => $q->notCanceled()
            )
            // Belum di pivot kontrak_pekerjaan
            ->whereDoesntHave('kontrak')
            // Belum sebagai id_pekerjaan utama di tbl_kontrak
            ->whereNotIn('id', function ($q) {
                $q->select('id_pekerjaan')
                    ->from('tbl_kontrak')
                    ->whereNotNull('id_pekerjaan');
            });

        if (Schema::hasColumn('tbl_pekerjaan', 'is_konsultan')) {
            $query->where(function ($q) {
                $q->where('is_konsultan', false)->orWhereNull('is_konsultan');
            });
        }

        if ($search) {
            $query->where(function ($q) use ($search) {
                $q->where('nama_paket', 'like', "%{$search}%")
                    ->orWhere('kode_rekening', 'like', "%{$search}%")
                    ->orWhereHas(
                        'kegiatan',
                        fn ($k) => $k->where('nama_sub_kegiatan', 'like', "%{$search}%")
                    );
            });
        }

        if ($subKegiatan) {
            $query->whereHas(
                'kegiatan',
                fn ($k) => $k->where('nama_sub_kegiatan', $subKegiatan)
            );
        }

        return $query
            ->orderBy('nama_paket')
            ->get()
            ->map(function (Pekerjaan $p) {
                return [
                    'pekerjaan_id' => $p->id,
                    'nama_paket' => $p->nama_paket,
                    'kode_rekening' => $p->kode_rekening,
                    'sub_kegiatan' => $p->kegiatan?->nama_sub_kegiatan,
                    'pagu' => (float) ($p->pagu ?? 0),
                    'kecamatan' => $p->kecamatan?->n_kec,
                    'desa' => $p->desa?->n_desa,
                ];
            })
            ->values()
            ->all();
    }

    private function calculateSummary($items): array
    {
        $total = $this->calculateAverage($items);
        $latestUpdatedAt = $items
            ->map(fn ($item) => $item->progress_fisik?->updated_at)
            ->filter()
            ->sortDesc()
            ->first();
        $perSubKegiatan = [];

        foreach ($items as $item) {
            $names = $this->subKegiatanNames($item);

            foreach ($names as $name) {
                $perSubKegiatan[$name] ??= [];
                $perSubKegiatan[$name][] = $item;
            }
        }

        $perSubKegiatan = collect($perSubKegiatan)
            ->map(fn ($group, $name) => [
                'sub_kegiatan' => $name,
                ...$this->calculateAverage(collect($group)),
            ])
            ->values()
            ->all();

        $withoutOutputs = $items
            ->filter(fn ($item) => $this->collectLinkedOutputs($item)->isEmpty())
            ->count();

        $summary = [
            ...$total,
            'latest_updated_at' => $latestUpdatedAt?->toISOString(),
            'per_sub_kegiatan' => $perSubKegiatan,
            'per_sub_kegiatan_output' => $this->calculateOutputSummaryPerSubKegiatan($items),
            'kontrak_tanpa_output' => $withoutOutputs,
        ];

        if (Schema::hasColumn('puspen_progress_fisik', 'pho_completed')) {
            $summary['pho_completed'] = $items
                ->filter(fn ($item) => (bool) ($item->progress_fisik?->pho_completed ?? false))
                ->count();
        }

        return $summary;
    }

    private function calculateOutputSummaryPerSubKegiatan($items): array
    {
        $perSub = [];

        foreach ($items as $kontrak) {
            $names = $this->subKegiatanNames($kontrak);
            $outputs = $this->collectLinkedOutputs($kontrak);
            $saved = $kontrak->relationLoaded('progress_fisik_outputs')
                ? $kontrak->progress_fisik_outputs->keyBy('output_id')
                : collect();

            foreach ($names as $name) {
                $perSub[$name] ??= [
                    'output_count' => 0,
                    'volume_target' => 0.0,
                    'volume_realisasi' => 0.0,
                    'komponen' => [],
                ];

                foreach ($outputs as $output) {
                    if (! $output || ! $output->id) {
                        continue;
                    }

                    $volume = (float) ($output->volume ?? 0);
                    $realisasi = $saved->get($output->id)?->realisasi;
                    $komponenKey = mb_strtolower(trim((string) $output->komponen))
                        .'||'
                        .mb_strtolower(trim((string) $output->satuan));

                    $perSub[$name]['output_count']++;
                    $perSub[$name]['volume_target'] += $volume;

                    if ($realisasi !== null) {
                        $perSub[$name]['volume_realisasi'] += (float) $realisasi;
                    }

                    $perSub[$name]['komponen'][$komponenKey] ??= [
                        'komponen' => $output->komponen,
                        'satuan' => $output->satuan,
                        'output_count' => 0,
                        'volume_target' => 0.0,
                        'volume_realisasi' => 0.0,
                    ];

                    $perSub[$name]['komponen'][$komponenKey]['output_count']++;
                    $perSub[$name]['komponen'][$komponenKey]['volume_target'] += $volume;

                    if ($realisasi !== null) {
                        $perSub[$name]['komponen'][$komponenKey]['volume_realisasi'] += (float) $realisasi;
                    }
                }
            }
        }

        return collect($perSub)
            ->map(function (array $stats, string $name) {
                $komponen = collect($stats['komponen'])
                    ->map(fn (array $row) => [
                        'komponen' => $row['komponen'],
                        'satuan' => $row['satuan'],
                        'output_count' => $row['output_count'],
                        'volume_target' => round($row['volume_target'], 2),
                        'volume_realisasi' => round($row['volume_realisasi'], 2),
                        'capaian' => $row['volume_target'] > 0
                            ? round(($row['volume_realisasi'] / $row['volume_target']) * 100, 2)
                            : null,
                    ])
                    ->sortBy('komponen')
                    ->values()
                    ->all();

                return [
                    'sub_kegiatan' => $name,
                    'output_count' => $stats['output_count'],
                    'volume_target' => round($stats['volume_target'], 2),
                    'volume_realisasi' => round($stats['volume_realisasi'], 2),
                    'capaian' => $stats['volume_target'] > 0
                        ? round(($stats['volume_realisasi'] / $stats['volume_target']) * 100, 2)
                        : null,
                    'komponen' => $komponen,
                ];
            })
            ->sortBy('sub_kegiatan')
            ->values()
            ->all();
    }

    private function collectLinkedOutputs(Kontrak $kontrak): Collection
    {
        $outputs = collect();

        if ($kontrak->relationLoaded('pekerjaan') && $kontrak->pekerjaan?->relationLoaded('output')) {
            $outputs = $outputs->merge($kontrak->pekerjaan->output);
        }

        if ($kontrak->relationLoaded('pekerjaans')) {
            foreach ($kontrak->pekerjaans as $pekerjaan) {
                if ($pekerjaan->relationLoaded('output')) {
                    $outputs = $outputs->merge($pekerjaan->output);
                }
            }
        }

        return $outputs
            ->filter(fn ($output) => $output && $output->id)
            ->unique('id')
            ->values();
    }

    private function calculateAverage($items): array
    {
        $count = max($items->count(), 1);
        $rencana = $items->sum(fn ($item) => (float) ($item->progress_fisik?->rencana ?? 0)) / $count;
        $realisasi = $items->sum(fn ($item) => (float) ($item->progress_fisik?->realisasi ?? 0)) / $count;

        return [
            'count' => $items->count(),
            'rencana' => round($rencana, 2),
            'realisasi' => round($realisasi, 2),
            'deviasi' => round($realisasi - $rencana, 2),
        ];
    }

    private function subKegiatanNames(Kontrak $kontrak): array
    {
        $names = collect([$kontrak->kegiatan?->nama_sub_kegiatan])
            ->merge($kontrak->pekerjaans->pluck('kegiatan.nama_sub_kegiatan'))
            ->filter()
            ->unique()
            ->values();

        return $names->isEmpty() ? ['Tanpa Sub Kegiatan'] : $names->all();
    }

    public function bulkUpdate(Request $request, PekerjaanProgressEstimasiSyncService $syncService)
    {
        $validated = $this->validateProgressPayload($request, true);

        foreach ($validated['items'] as $item) {
            $kontrakId = (int) $item['kontrak_id'];
            $outputItems = $item['outputs'] ?? [];

            if (! empty($outputItems)) {
                $this->persistOutputRealisasi($kontrakId, (int) $validated['tahun'], $outputItems);
            }

            $rencana = array_key_exists('rencana', $item) ? $item['rencana'] : null;
            $realisasi = array_key_exists('realisasi', $item) ? $item['realisasi'] : null;
            $progressPayload = [
                'rencana' => $rencana,
                'realisasi' => $realisasi,
            ];

            if (
                Schema::hasColumn('puspen_progress_fisik', 'pho_completed')
                && array_key_exists('pho_completed', $item)
            ) {
                $progressPayload['pho_completed'] = (bool) $item['pho_completed'];
            }

            PuspenProgressFisik::updateOrCreate(
                [
                    'kontrak_id' => $kontrakId,
                    'tahun_anggaran' => $validated['tahun'],
                ],
                $progressPayload
            );

            $syncService->syncFromPuspenKontrak(
                $kontrakId,
                (int) $validated['tahun'],
                $rencana,
                $realisasi,
            );
        }

        return response()->json(['message' => 'Progress fisik berhasil disimpan']);
    }

    /**
     * @param  array<int, array<string, mixed>>  $outputItems
     */
    private function persistOutputRealisasi(int $kontrakId, int $tahun, array $outputItems): void
    {
        if (! Schema::hasTable('puspen_progress_fisik_output')) {
            return;
        }

        foreach ($outputItems as $outputItem) {
            $outputId = (int) $outputItem['output_id'];
            $realisasi = $outputItem['realisasi'] ?? null;

            if ($realisasi === null || $realisasi === '') {
                PuspenProgressFisikOutput::query()
                    ->where('kontrak_id', $kontrakId)
                    ->where('output_id', $outputId)
                    ->where('tahun_anggaran', $tahun)
                    ->delete();

                continue;
            }

            PuspenProgressFisikOutput::updateOrCreate(
                [
                    'kontrak_id' => $kontrakId,
                    'output_id' => $outputId,
                    'tahun_anggaran' => $tahun,
                ],
                [
                    'realisasi' => $realisasi,
                ]
            );
        }
    }

    private function validateProgressPayload(Request $request, bool $needsYear): array
    {
        $rules = [
            'items' => 'required|array',
            'items.*.kontrak_id' => 'required|integer|exists:tbl_kontrak,id',
            'items.*.rencana' => ['nullable', function ($attribute, $value, $fail) {
                $this->validatePercentInput($attribute, $value, $fail);
            }],
            'items.*.realisasi' => ['nullable', function ($attribute, $value, $fail) {
                $this->validatePercentInput($attribute, $value, $fail);
            }],
            'items.*.pho_completed' => 'nullable|boolean',
            'items.*.outputs' => 'nullable|array',
            'items.*.outputs.*.output_id' => 'required_with:items.*.outputs|integer|exists:tbl_output,id',
            'items.*.outputs.*.realisasi' => ['nullable', function ($attribute, $value, $fail) {
                $this->validateVolumeInput($attribute, $value, $fail);
            }],
        ];

        if ($needsYear) {
            $rules['tahun'] = 'required|integer|min:2000|max:2100';
        }

        $validated = $request->validate($rules);

        foreach ($validated['items'] as $index => $item) {
            if (empty($item['outputs'])) {
                continue;
            }

            $kontrak = Kontrak::query()
                ->with(['pekerjaan.output:id,pekerjaan_id', 'pekerjaans.output:id,pekerjaan_id'])
                ->find($item['kontrak_id']);

            if (! $kontrak) {
                continue;
            }

            $allowedOutputIds = $this->resolveAllowedOutputIds($kontrak);
            foreach ($item['outputs'] as $outputIndex => $outputItem) {
                $outputId = (int) $outputItem['output_id'];
                if (! $allowedOutputIds->contains($outputId)) {
                    throw \Illuminate\Validation\ValidationException::withMessages([
                        "items.{$index}.outputs.{$outputIndex}.output_id" => 'Output tidak terhubung dengan kontrak ini.',
                    ]);
                }
            }
        }

        return $validated;
    }

    private function resolveAllowedOutputIds(Kontrak $kontrak): Collection
    {
        $outputs = collect();

        if ($kontrak->relationLoaded('pekerjaan') && $kontrak->pekerjaan?->relationLoaded('output')) {
            $outputs = $outputs->merge($kontrak->pekerjaan->output->pluck('id'));
        }

        if ($kontrak->relationLoaded('pekerjaans')) {
            foreach ($kontrak->pekerjaans as $pekerjaan) {
                if ($pekerjaan->relationLoaded('output')) {
                    $outputs = $outputs->merge($pekerjaan->output->pluck('id'));
                }
            }
        }

        return $outputs->unique()->values();
    }

    private function validatePercentInput(string $attribute, mixed $value, \Closure $fail): void
    {
        if ($value === null || $value === '') {
            return;
        }

        $stringValue = trim((string) $value);

        if (preg_match('/^\d+([.,]\d{1,2})?$/', $stringValue) !== 1) {
            $fail("Field {$attribute} harus berupa angka desimal dengan maksimal 2 angka di belakang koma.");
            return;
        }

        $normalized = str_replace(',', '.', $stringValue);
        if (! is_numeric($normalized)) {
            $fail("Field {$attribute} harus berupa angka desimal.");
            return;
        }

        $numeric = (float) $normalized;
        if ($numeric < 0 || $numeric > 100) {
            $fail("Field {$attribute} harus berada antara 0 dan 100.");
        }
    }

    private function validateVolumeInput(string $attribute, mixed $value, \Closure $fail): void
    {
        if ($value === null || $value === '') {
            return;
        }

        $stringValue = trim((string) $value);

        if (preg_match('/^\d+([.,]\d{1,2})?$/', $stringValue) !== 1) {
            $fail("Field {$attribute} harus berupa angka desimal dengan maksimal 2 angka di belakang koma.");
            return;
        }

        $normalized = str_replace(',', '.', $stringValue);
        if (! is_numeric($normalized)) {
            $fail("Field {$attribute} harus berupa angka desimal.");
            return;
        }

        if ((float) $normalized < 0) {
            $fail("Field {$attribute} tidak boleh kurang dari 0.");
        }
    }
}