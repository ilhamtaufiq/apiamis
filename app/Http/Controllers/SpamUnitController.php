<?php

namespace App\Http\Controllers;

use App\Models\Foto;
use App\Models\Pekerjaan;
use App\Models\UnitSpam;
use App\Models\Desa;
use App\Models\Kecamatan;
use App\Models\Pengelola;
use App\Models\SpamAchievement;
use App\Services\SpamPekerjaanIntegrationService;
use Illuminate\Http\Request;
use Illuminate\Http\JsonResponse;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Artisan;
use Illuminate\Support\Facades\Storage;

class SpamUnitController extends Controller
{
    public function __construct(
        private readonly SpamPekerjaanIntegrationService $integrationService
    ) {
    }
    public function import(Request $request): JsonResponse
    {
        $request->validate([
            'file' => 'required|file|mimes:csv,txt|max:10240', // 10MB max
        ]);

        try {
            $file = $request->file('file');
            $path = $file->storeAs('temp', 'import_spam_data.csv');
            $fullPath = storage_path('app/' . $path);

            Artisan::call('spam:import-data', [
                '--file' => $fullPath
            ]);

            $output = Artisan::output();

            return response()->json([
                'message' => 'Data successfully imported',
                'output' => $output
            ]);
        } catch (\Exception $e) {
            return response()->json([
                'message' => 'Failed to import data',
                'error' => $e->getMessage()
            ], 500);
        }
    }

    public function index(Request $request): JsonResponse
    {
        $query = UnitSpam::with(['desa.kecamatan', 'pengelola', 'budgets' => function($q) {
            $q->orderBy('tahun', 'desc');
        }, 'achievements' => function ($q) use ($request) {
            if ($request->filled('tahun')) {
                $q->where('tahun', $request->tahun);
            }
            $q->orderBy('tahun', 'desc');
        }]);

        // Filter by kecamatan
        if ($request->filled('kecamatan_id')) {
            $query->whereHas('desa', function ($q) use ($request) {
                $q->where('kecamatan_id', $request->kecamatan_id);
            });
        }

        // Filter by desa
        if ($request->filled('desa_id')) {
            $query->where('desa_id', $request->desa_id);
        }

        // Filter by SIMSPAM
        if ($request->has('is_simspam') && $request->is_simspam !== '') {
            $query->where('is_simspam', filter_var($request->is_simspam, FILTER_VALIDATE_BOOLEAN));
        }

        // Filter by search
        if ($request->filled('search')) {
            $search = '%' . $request->search . '%';
            $query->where(function ($q) use ($search) {
                $q->where('name', 'like', $search)
                  ->orWhere('sistem_layanan', 'like', $search)
                  ->orWhereHas('desa', function ($dq) use ($search) {
                      $dq->where('n_desa', 'like', $search);
                  })
                  ->orWhereHas('pengelola', function ($pq) use ($search) {
                      $pq->where('pokmas', 'like', $search)
                        ->orWhere('kepala', 'like', $search);
                  });
            });
        }

        $units = $query->paginate($request->integer('per_page', 15));

        return response()->json([
            'success' => true,
            'data' => $units->items(),
            'meta' => [
                'current_page' => $units->currentPage(),
                'last_page' => $units->lastPage(),
                'per_page' => $units->perPage(),
                'total' => $units->total(),
            ]
        ]);
    }

    public function show(UnitSpam $spamUnit): JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => $spamUnit->load([
                'desa.kecamatan',
                'pengelola',
                'pekerjaan.kegiatan',
                'pekerjaan.output',
                'pekerjaan.kontrak',
                'budgets' => function ($q) {
                    $q->orderBy('tahun', 'desc');
                },
                'achievements' => function ($q) {
                    $q->orderBy('tahun', 'desc');
                },
                'checklists',
            ]),
        ]);
    }

    public function store(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'desa_id' => 'required|exists:tbl_desa,id',
            'name' => 'nullable|string|max:255',
            'is_simspam' => 'required|boolean',
            'sistem_layanan' => 'nullable|string|max:255',
            'sumber_mata_air_kap' => 'nullable|string|max:255',
            'sumber_air_tanah_kap' => 'nullable|string|max:255',
            'lain_lain_kap' => 'nullable|string|max:255',
            // Pengelola
            'pokmas' => 'nullable|string|max:255',
            'perdes' => 'nullable|string|max:255',
            'kepala' => 'nullable|string|max:255',
            'bendahara' => 'nullable|string|max:255',
            'sekretaris' => 'nullable|string|max:255',
        ]);

        $unit = UnitSpam::create($validated);

        // Create pengelola
        $unit->pengelola()->create([
            'pokmas' => $request->pokmas,
            'perdes' => $request->perdes,
            'kepala' => $request->kepala,
            'bendahara' => $request->bendahara,
            'sekretaris' => $request->sekretaris,
        ]);

        return response()->json([
            'success' => true,
            'data' => $unit->load('pengelola'),
            'message' => 'Unit SPAM berhasil ditambahkan'
        ], 201);
    }

    public function update(Request $request, UnitSpam $spamUnit): JsonResponse
    {
        $validated = $request->validate([
            'desa_id' => 'required|exists:tbl_desa,id',
            'name' => 'nullable|string|max:255',
            'is_simspam' => 'required|boolean',
            'sistem_layanan' => 'nullable|string|max:255',
            'sumber_mata_air_kap' => 'nullable|string|max:255',
            'sumber_air_tanah_kap' => 'nullable|string|max:255',
            'lain_lain_kap' => 'nullable|string|max:255',
            // Pengelola
            'pokmas' => 'nullable|string|max:255',
            'perdes' => 'nullable|string|max:255',
            'kepala' => 'nullable|string|max:255',
            'bendahara' => 'nullable|string|max:255',
            'sekretaris' => 'nullable|string|max:255',
        ]);

        $spamUnit->update($validated);

        // Update or Create Pengelola
        $pengelola = $spamUnit->pengelola;
        
        if ($pengelola) {
            $pengelola->pokmas = $request->pokmas;
            $pengelola->perdes = $request->perdes;
            $pengelola->kepala = $request->kepala;
            $pengelola->bendahara = $request->bendahara;
            $pengelola->sekretaris = $request->sekretaris;
            $pengelola->save();
        } else {
            $newPengelola = new \App\Models\Pengelola();
            $newPengelola->unit_spam_id = $spamUnit->id;
            $newPengelola->pokmas = $request->pokmas;
            $newPengelola->perdes = $request->perdes;
            $newPengelola->kepala = $request->kepala;
            $newPengelola->bendahara = $request->bendahara;
            $newPengelola->sekretaris = $request->sekretaris;
            $newPengelola->save();
        }


        return response()->json([
            'success' => true,
            'data' => $spamUnit->load('pengelola'),
            'message' => 'Unit SPAM berhasil diperbarui'
        ]);
    }

    public function destroy(UnitSpam $spamUnit): JsonResponse
    {
        $spamUnit->delete();

        return response()->json([
            'success' => true,
            'message' => 'Unit SPAM berhasil dihapus'
        ]);
    }

    public function publicStats(Request $request): JsonResponse
    {
        return $this->stats($request);
    }

    public function publicMapStats(Request $request): JsonResponse
    {
        $tahun = $request->filled('tahun') ? $request->input('tahun') : null;

        return response()->json([
            'success' => true,
            'data' => $this->integrationService->desaMapStats($tahun),
        ]);
    }

    public function stats(Request $request): JsonResponse
    {
        // Simple aggregate calculations
        $totalUnits = UnitSpam::count();
        $simspamCount = UnitSpam::where('is_simspam', true)->count();
        $nonSimspamCount = $totalUnits - $simspamCount;

        $tahunScope = $request->filled('tahun') ? $request->input('tahun') : null;
        $scopeLabel = $this->integrationService->combinedScopeLabel($tahunScope);
        $targetYear = $scopeLabel;
        $manualScopeLabel = $scopeLabel;
        $manualCapTahun = $this->integrationService->manualCapTahun();

        // Filter by kecamatan if present
        $kecamatanId = $request->kecamatan_id ? (int) $request->kecamatan_id : null;

        $integrationSummary = $this->integrationService->integrationSummary(
            $tahunScope,
            $kecamatanId
        );

        $statsEnrichment = $this->integrationService->buildStatsEnrichment($tahunScope, $kecamatanId);

        $manualGlobal = $this->integrationService->aggregateManualGlobal($tahunScope, $kecamatanId);

        $targetQuery = Desa::query()->realWilayah();
        $achievementQuery = SpamAchievement::query();

        if ($tahunScope) {
            $achievementQuery->where('tahun', $tahunScope);
        } else {
            $achievementQuery->where(function ($query) {
                $query->where('tahun', '<=', SpamPekerjaanIntegrationService::BASELINE_CAP_TAHUN)
                    ->orWhere('tahun', '>=', SpamPekerjaanIntegrationService::ACCUMULATION_START_TAHUN);
            });
        }

        if ($kecamatanId) {
            $targetQuery->where('kecamatan_id', $kecamatanId);
            $achievementQuery->whereHas('unitSpam.desa', function ($q) use ($kecamatanId) {
                $q->where('kecamatan_id', $kecamatanId);
            });
        }

        $totalTarget = (int) $targetQuery->sum('target');
        $bjpMasterKk = (int) $targetQuery->sum('bjp_master');
        $bjpUnitKk = (int) $achievementQuery->sum('jumlah_bjp_kk');

        if ($tahunScope) {
            $totalSR = $manualGlobal['sr'];
            $totalKK = $manualGlobal['kk'];
            $totalJiwa = $manualGlobal['jiwa'];
            $displayNilaiKontrak = $manualGlobal['nilai_kontrak'];
        } else {
            $totalSR = $statsEnrichment['capaian_sr'];
            $totalKK = $statsEnrichment['capaian_kk'];
            $totalJiwa = $statsEnrichment['capaian_jiwa'];
            $displayNilaiKontrak = $statsEnrichment['capaian_nilai_kontrak'];
        }

        $totalBjpKK = $bjpMasterKk + $bjpUnitKk;
        $totalBjpJiwa = $totalBjpKK * 5;
        $coveragePercentage = $totalTarget > 0
            ? round((($totalKK + $totalBjpKK) / $totalTarget) * 100, 2)
            : 0;

        // Funding distribution (all years when no filter; scoped year when filtered)
        $fundingQuery = \App\Models\SpamBudget::select('sumber_dana', DB::raw('count(DISTINCT unit_spam_id) as count'));
        if ($tahunScope) {
            $fundingQuery->where('tahun', $tahunScope);
        } else {
            $fundingQuery->where(function ($query) {
                $query->where('tahun', '<=', SpamPekerjaanIntegrationService::BASELINE_CAP_TAHUN)
                    ->orWhere('tahun', '>=', SpamPekerjaanIntegrationService::ACCUMULATION_START_TAHUN);
            });
        }
        if ($kecamatanId) {
            $fundingQuery->whereHas('unitSpam.desa', function ($q) use ($kecamatanId) {
                $q->where('kecamatan_id', $kecamatanId);
            });
        }
        $fundingDist = $fundingQuery->groupBy('sumber_dana')
            ->orderBy('count', 'desc')
            ->get();

        return response()->json([
            'success' => true,
            'data' => array_merge([
                'total_units' => $totalUnits,
                'simspam_count' => $simspamCount,
                'non_simspam_count' => $nonSimspamCount,
                'target_year' => $targetYear,
                'manual_scope_label' => $manualScopeLabel,
                'manual_cap_tahun' => $manualCapTahun,
                'total_target' => $totalTarget,
                'total_sr' => $totalSR,
                'total_kk' => $totalKK,
                'total_jiwa' => $totalJiwa,
                'total_bjp_kk' => $totalBjpKK,
                'total_bjp_jiwa' => $totalBjpJiwa,
                'funding_distribution' => $fundingDist,
                'coverage_percentage' => $coveragePercentage,
                'wilayah_total_desa' => Desa::query()->realWilayah()->count(),
                'wilayah_total_kecamatan' => Kecamatan::query()->realWilayah()->count(),
                'achievement_records' => SpamAchievement::count(),
                'total_pekerjaan_all' => Pekerjaan::count(),
                'total_foto_dokumentasi' => Foto::count(),
                'stats_generated_at' => now()->toIso8601String(),
            ], $integrationSummary, $statsEnrichment, [
                'manual_sr' => $manualGlobal['sr'],
                'manual_kk' => $manualGlobal['kk'],
                'manual_jiwa' => $manualGlobal['jiwa'],
                'manual_nilai_kontrak' => $displayNilaiKontrak,
                'total_sr' => $totalSR,
                'total_kk' => $totalKK,
                'total_jiwa' => $totalJiwa,
                'total_linked' => $integrationSummary['total_linked'] ?? 0,
                'ringkasan' => [
                    'scope_label' => $scopeLabel,
                    'baseline_cap_tahun' => $statsEnrichment['baseline_cap_tahun'],
                    'accumulation_start_tahun' => $statsEnrichment['accumulation_start_tahun'],
                    'baseline' => [
                        'label' => 'Acuan master s/d '.$statsEnrichment['baseline_cap_tahun'],
                        'keterangan' => 'Data awal unit SPAM (import). Tidak ditimpa oleh integrasi pekerjaan.',
                        'sr' => $statsEnrichment['capaian_baseline_sr'],
                        'kk' => $statsEnrichment['capaian_baseline_kk'],
                        'jiwa' => $statsEnrichment['capaian_baseline_jiwa'],
                        'nilai_kontrak' => $statsEnrichment['capaian_baseline_nilai_kontrak'],
                    ],
                    'capaian' => [
                        'label' => 'Capaian unit SPAM tercatat (total)',
                        'keterangan' => 'Acuan s/d '.$statsEnrichment['baseline_cap_tahun'].' + capaian integrasi '.$statsEnrichment['accumulation_start_tahun'].' ke atas',
                        'sr' => $statsEnrichment['capaian_sr'],
                        'kk' => $statsEnrichment['capaian_kk'],
                        'jiwa' => $statsEnrichment['capaian_jiwa'],
                        'nilai_kontrak' => $statsEnrichment['capaian_nilai_kontrak'],
                    ],
                    'integrasi' => [
                        'label' => 'Status tautan pekerjaan',
                        'paket_tertaut' => $statsEnrichment['linked_pekerjaan_count'],
                        'paket_tersedia' => $integrationSummary['pekerjaan_air_minum_count'] ?? 0,
                        'paket_belum_tertaut' => $statsEnrichment['paket_belum_tertaut'],
                        'unit_dengan_tautan' => $statsEnrichment['linked_units_count'],
                        'desa_terintegrasi' => $integrationSummary['matched_count'] ?? 0,
                        'desa_partial' => $integrationSummary['partial_count'] ?? 0,
                        'desa_tanpa_unit' => $integrationSummary['no_unit_count'] ?? 0,
                        'desa_tanpa_pekerjaan' => $integrationSummary['no_pekerjaan_count'] ?? 0,
                    ],
                    'capaian_integrasi' => [
                        'label' => 'Capaian integrasi '.$statsEnrichment['accumulation_start_tahun'].' ke atas',
                        'keterangan' => 'Hanya tahun integrasi; dipakai untuk perbandingan dengan potensi pekerjaan',
                        'sr' => $statsEnrichment['capaian_integrasi_sr'],
                        'kk' => $statsEnrichment['capaian_integrasi_kk'],
                        'jiwa' => $statsEnrichment['capaian_integrasi_jiwa'],
                        'nilai_kontrak' => $statsEnrichment['capaian_integrasi_nilai_kontrak'],
                    ],
                    'potensi' => [
                        'label' => 'Potensi pekerjaan AM ('.$statsEnrichment['accumulation_start_tahun'].' ke atas)',
                        'keterangan' => 'Paket sub bidang air minum tahun '.$statsEnrichment['accumulation_start_tahun'].' ke atas (belum tentu sudah ditaut)',
                        'sr' => $statsEnrichment['potensi_sr'],
                        'kk' => $statsEnrichment['potensi_kk'],
                        'jiwa' => $statsEnrichment['potensi_jiwa'],
                        'nilai_kontrak' => $statsEnrichment['potensi_nilai_kontrak'],
                    ],
                    'dari_tautan' => [
                        'label' => 'Akumulasi paket yang sudah ditaut',
                        'sr' => $statsEnrichment['linked_sr'],
                        'kk' => $statsEnrichment['linked_kk'],
                        'jiwa' => $statsEnrichment['linked_jiwa'],
                        'nilai_kontrak' => $statsEnrichment['linked_nilai_kontrak'],
                    ],
                    'selisih_potensi_capaian' => [
                        'sr' => $statsEnrichment['selisih_sr'],
                        'kk' => $statsEnrichment['selisih_kk'],
                        'jiwa' => $statsEnrichment['selisih_jiwa'],
                        'nilai_kontrak' => $statsEnrichment['selisih_nilai_kontrak'],
                    ],
                    'spm' => [
                        'target_kk' => $totalTarget,
                        'jp_kk' => $totalKK,
                        'bjp_master_kk' => $bjpMasterKk,
                        'bjp_unit_kk' => $bjpUnitKk,
                        'total_bjp_kk' => $totalBjpKK,
                        'coverage_percentage' => $coveragePercentage,
                    ],
                ],
            ]),
        ]);
    }

    public function integrationOutputOptions(Request $request): JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => $this->integrationService->listIntegrationOutputOptions(
                $request->filled('tahun') ? $request->input('tahun') : null,
                $request->filled('kecamatan_id') ? $request->integer('kecamatan_id') : null,
            ),
        ]);
    }

    public function integration(Request $request): JsonResponse
    {
        $request->validate([
            'sync_status' => 'nullable|in:matched,partial,no_unit,no_pekerjaan,no_data',
            'output_type' => 'nullable|in:sambungan_rumah,pipa_jaringan,reservoir,sumber_air,bjp',
            'komponen' => 'nullable|string|max:255',
        ]);

        $result = $this->integrationService->paginateIntegration(
            $request->filled('tahun') ? $request->input('tahun') : null,
            $request->filled('kecamatan_id') ? $request->integer('kecamatan_id') : null,
            $request->filled('desa_id') ? $request->integer('desa_id') : null,
            $request->filled('search') ? $request->input('search') : null,
            $request->filled('sync_status') ? $request->input('sync_status') : null,
            $request->filled('output_type') ? $request->input('output_type') : null,
            $request->filled('komponen') ? $request->input('komponen') : null,
            $request->integer('per_page', 15),
            $request->integer('page', 1)
        );

        return response()->json([
            'success' => true,
            'data' => $result['data'],
            'meta' => $result['meta'],
            'summary' => $result['summary'],
        ]);
    }

    public function integrationByDesa(int $desaId, Request $request): JsonResponse
    {
        $request->validate([
            'output_type' => 'nullable|in:sambungan_rumah,pipa_jaringan,reservoir,sumber_air,bjp',
            'komponen' => 'nullable|string|max:255',
        ]);

        $desa = Desa::with('kecamatan')->findOrFail($desaId);

        return response()->json([
            'success' => true,
            'data' => $this->integrationService->buildDesaIntegrationRow(
                $desa,
                $request->filled('tahun') ? $request->input('tahun') : null,
                $request->filled('output_type') ? $request->input('output_type') : null,
                $request->filled('komponen') ? $request->input('komponen') : null,
            ),
        ]);
    }

    public function airMinumPekerjaan(Request $request): JsonResponse
    {
        $request->validate([
            'output_type' => 'nullable|in:sambungan_rumah,pipa_jaringan,reservoir,sumber_air,bjp',
        ]);

        $result = $this->integrationService->paginateAirMinumPekerjaan(
            $request->filled('tahun') ? $request->input('tahun') : null,
            $request->filled('kecamatan_id') ? $request->integer('kecamatan_id') : null,
            $request->filled('desa_id') ? $request->integer('desa_id') : null,
            $request->filled('search') ? $request->input('search') : null,
            $request->filled('output_type') ? $request->input('output_type') : null,
            $request->filled('unit_spam_id') ? $request->integer('unit_spam_id') : null,
            $request->boolean('unlinked_only') ?: null,
            $request->integer('per_page', 15),
            $request->integer('page', 1),
        );

        return response()->json([
            'success' => true,
            'data' => $result['data'],
            'meta' => $result['meta'],
        ]);
    }

    public function attachPekerjaan(Request $request, UnitSpam $unitSpam): JsonResponse
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'output_id' => 'nullable|exists:tbl_output,id',
            'capaian_metric' => 'nullable|in:jp,bjp',
        ]);

        try {
            $this->integrationService->attachPekerjaan(
                $unitSpam,
                (int) $validated['pekerjaan_id'],
                isset($validated['output_id']) ? (int) $validated['output_id'] : null,
                $validated['capaian_metric'] ?? null,
            );
        } catch (\InvalidArgumentException $e) {
            return response()->json(['success' => false, 'message' => $e->getMessage()], 422);
        }

        $unitSpam->refresh()->load([
            'desa.kecamatan',
            'pengelola',
            'pekerjaan.kegiatan',
            'pekerjaan.output',
            'pekerjaan.kontrak',
            'achievements',
            'budgets',
        ]);

        $pekerjaan = Pekerjaan::query()
            ->with('kegiatan')
            ->find((int) $validated['pekerjaan_id']);
        $tahunAnggaran = (string) ($pekerjaan?->kegiatan?->tahun_anggaran ?? '');
        $startTahun = $this->integrationService->accumulationStartTahun();

        $message = SpamPekerjaanIntegrationService::isAccumulationTahun($tahunAnggaran)
            ? "Pekerjaan berhasil ditautkan. Capaian SR/KK/jiwa dan anggaran tahun {$tahunAnggaran} diakumulasi ke unit (kontrak, atau pagu jika kontrak kosong)."
            : "Pekerjaan berhasil ditautkan sebagai referensi. Capaian unit s/d {$this->integrationService->baselineCapTahun()} tidak diubah; akumulasi otomatis berlaku mulai tahun {$startTahun}.";

        return response()->json([
            'success' => true,
            'message' => $message,
            'data' => $unitSpam,
        ]);
    }

    public function detachPekerjaan(UnitSpam $unitSpam, int $pekerjaanId): JsonResponse
    {
        $this->integrationService->detachPekerjaan($unitSpam, $pekerjaanId);

        $unitSpam->refresh();

        return response()->json([
            'success' => true,
            'message' => 'Tautan pekerjaan berhasil dihapus. Akumulasi capaian dan anggaran disesuaikan ulang.',
            'data' => $unitSpam,
        ]);
    }

    public function syncPekerjaan(Request $request, UnitSpam $unitSpam): JsonResponse
    {
        $validated = $request->validate([
            'tahun' => 'required|string|max:4',
            'mode' => 'required|in:achievement,budget,all',
        ]);

        $data = $this->integrationService->syncToUnit(
            $unitSpam,
            $validated['tahun'],
            $validated['mode']
        );

        return response()->json([
            'success' => true,
            'message' => 'Data pekerjaan berhasil disinkronkan ke unit SPAM',
            'data' => $data,
        ]);
    }

    public function addAchievement(Request $request, UnitSpam $unitSpam): JsonResponse
    {
        $validated = $request->validate([
            'tahun' => 'required|string|max:4',
            'jumlah_sr' => 'required|integer|min:0',
            'jumlah_kk' => 'required|integer|min:0',
            'jumlah_jiwa' => 'required|integer|min:0',
            'jumlah_bjp_kk' => 'nullable|integer|min:0',
            'jumlah_bjp_jiwa' => 'nullable|integer|min:0',
            'catatan' => 'nullable|string',
        ]);

        $validated['jumlah_bjp_kk'] = $validated['jumlah_bjp_kk'] ?? 0;
        $validated['jumlah_bjp_jiwa'] = $validated['jumlah_bjp_jiwa'] ?? ($validated['jumlah_bjp_kk'] * 5);

        $achievement = $unitSpam->achievements()->updateOrCreate(
            ['tahun' => $validated['tahun']],
            $validated
        );

        return response()->json([
            'success' => true,
            'data' => $achievement,
            'message' => 'Histori achievement berhasil ditambahkan!'
        ], 201);
    }

    public function addBudget(Request $request, UnitSpam $unitSpam): JsonResponse
    {
        $validated = $request->validate([
            'tahun' => 'required|string|max:4',
            'nilai_kontrak' => 'required|numeric|min:0',
            'nama_paket' => 'nullable|string|max:255',
            'sumber_dana' => 'nullable|string|max:255',
        ]);

        $budget = $unitSpam->budgets()->create($validated);

        return response()->json([
            'success' => true,
            'data' => $budget,
            'message' => 'Data anggaran berhasil ditambahkan!'
        ], 201);
    }

    public function deleteBudget(UnitSpam $unitSpam, $budgetId): JsonResponse
    {
        $budget = $unitSpam->budgets()->findOrFail($budgetId);
        $budget->delete();

        return response()->json([
            'success' => true,
            'message' => 'Data anggaran berhasil dihapus!'
        ]);
    }
}
