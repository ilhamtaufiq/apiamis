<?php

namespace App\Http\Controllers;

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
            'data' => $spamUnit->load(['desa.kecamatan', 'pengelola', 'budgets' => function($q) {
                $q->orderBy('tahun', 'desc');
            }, 'achievements' => function($q) {
                $q->orderBy('tahun', 'desc');
            }, 'checklists'])
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
        $targetYear = $tahunScope ?? 's/d '.$this->integrationService->manualCapTahun();
        $manualScopeLabel = $this->integrationService->manualScopeLabel($tahunScope);
        $manualCapTahun = $this->integrationService->manualCapTahun();

        // Filter by kecamatan if present
        $kecamatanId = $request->kecamatan_id ? (int) $request->kecamatan_id : null;

        $integrationSummary = $this->integrationService->integrationSummary(
            $tahunScope,
            $kecamatanId
        );

        $manualGlobal = $this->integrationService->aggregateManualGlobal($tahunScope, $kecamatanId);

        $targetQuery = Desa::query();
        $achievementQuery = SpamAchievement::query();

        if ($tahunScope) {
            $achievementQuery->where('tahun', $tahunScope);
        } else {
            $achievementQuery->where('tahun', '<=', $manualCapTahun);
        }

        if ($kecamatanId) {
            $targetQuery->where('kecamatan_id', $kecamatanId);
            $achievementQuery->whereHas('unitSpam.desa', function ($q) use ($kecamatanId) {
                $q->where('kecamatan_id', $kecamatanId);
            });
        }

        $totalTarget = (int) $targetQuery->sum('target');

        $totalSR = $manualGlobal['sr'];
        $totalKK = $manualGlobal['kk'];
        $totalJiwa = $manualGlobal['jiwa'];

        $totalBjpKK = (int) $targetQuery->sum('bjp_master') + (int) $achievementQuery->sum('jumlah_bjp_kk');
        $totalBjpJiwa = $totalBjpKK * 5;

        // Funding distribution (all years when no filter; scoped year when filtered)
        $fundingQuery = \App\Models\SpamBudget::select('sumber_dana', DB::raw('count(DISTINCT unit_spam_id) as count'));
        if ($tahunScope) {
            $fundingQuery->where('tahun', $tahunScope);
        } else {
            $fundingQuery->where('tahun', '<=', $manualCapTahun);
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
                'coverage_percentage' => $totalTarget > 0 ? round((($totalKK + $totalBjpKK) / $totalTarget) * 100, 2) : 0,
            ], $integrationSummary, [
                'manual_sr' => $manualGlobal['sr'],
                'manual_kk' => $manualGlobal['kk'],
                'manual_jiwa' => $manualGlobal['jiwa'],
                'manual_nilai_kontrak' => $manualGlobal['nilai_kontrak'],
                'total_sr' => $totalSR,
                'total_kk' => $totalKK,
                'total_jiwa' => $totalJiwa,
            ]),
        ]);
    }

    public function integration(Request $request): JsonResponse
    {
        $request->validate([
            'sync_status' => 'nullable|in:matched,partial,no_unit,no_pekerjaan',
        ]);

        $result = $this->integrationService->paginateIntegration(
            $request->filled('tahun') ? $request->input('tahun') : null,
            $request->filled('kecamatan_id') ? $request->integer('kecamatan_id') : null,
            $request->filled('desa_id') ? $request->integer('desa_id') : null,
            $request->filled('search') ? $request->input('search') : null,
            $request->filled('sync_status') ? $request->input('sync_status') : null,
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
        $desa = Desa::with('kecamatan')->findOrFail($desaId);

        return response()->json([
            'success' => true,
            'data' => $this->integrationService->buildDesaIntegrationRow(
                $desa,
                $request->filled('tahun') ? $request->input('tahun') : null
            ),
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
