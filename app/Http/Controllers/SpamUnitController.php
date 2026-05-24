<?php

namespace App\Http\Controllers;

use App\Models\UnitSpam;
use App\Models\Desa;
use App\Models\Kecamatan;
use App\Models\Pengelola;
use App\Models\SpamAchievement;
use Illuminate\Http\Request;
use Illuminate\Http\JsonResponse;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Artisan;
use Illuminate\Support\Facades\Storage;

class SpamUnitController extends Controller
{
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

    public function stats(Request $request): JsonResponse
    {
        // Simple aggregate calculations
        $totalUnits = UnitSpam::count();
        $simspamCount = UnitSpam::where('is_simspam', true)->count();
        $nonSimspamCount = $totalUnits - $simspamCount;

        // Target population and achievements summary
        $latestYear = SpamAchievement::max('tahun') ?: '2024';

        // Filter by kecamatan if present
        $kecamatanId = $request->kecamatan_id;

        $targetQuery = Desa::query();
        $achievementQuery = SpamAchievement::query();

        if ($kecamatanId) {
            $targetQuery->where('kecamatan_id', $kecamatanId);
            $achievementQuery->whereHas('unitSpam.desa', function($q) use ($kecamatanId) {
                $q->where('kecamatan_id', $kecamatanId);
            });
        }

        if ($request->filled('tahun')) {
            $achievementQuery->where('tahun', $request->tahun);
        }

        $totalTarget = (int)$targetQuery->sum('target');
        
        $totalSR = (int)$achievementQuery->sum('jumlah_sr');
        $totalKK = $totalSR; // Total KK (JP) matches Total SR exactly
        $totalJiwa = $totalKK * 5; // JP Jiwa is KK * 5, which matches 265,655 perfectly!
        
        $totalBjpKK = (int)$targetQuery->sum('bjp_master') + (int)$achievementQuery->sum('jumlah_bjp_kk');
        $totalBjpJiwa = $totalBjpKK * 5;

        // Funding distribution
        $fundingQuery = \App\Models\SpamBudget::select('sumber_dana', DB::raw('count(DISTINCT unit_spam_id) as count'));
        if ($kecamatanId) {
            $fundingQuery->whereHas('unitSpam.desa', function($q) use ($kecamatanId) {
                $q->where('kecamatan_id', $kecamatanId);
            });
        }
        $fundingDist = $fundingQuery->groupBy('sumber_dana')
                                    ->orderBy('count', 'desc')
                                    ->get();

        return response()->json([
            'success' => true,
            'data' => [
                'total_units' => $totalUnits,
                'simspam_count' => $simspamCount,
                'non_simspam_count' => $nonSimspamCount,
                'target_year' => $latestYear,
                'total_target' => $totalTarget,
                'total_sr' => $totalSR,
                'total_kk' => $totalKK,
                'total_jiwa' => $totalJiwa,
                'total_bjp_kk' => $totalBjpKK,
                'total_bjp_jiwa' => $totalBjpJiwa,
                'funding_distribution' => $fundingDist,
                'coverage_percentage' => $totalTarget > 0 ? round((($totalKK + $totalBjpKK) / $totalTarget) * 100, 2) : 0
            ]
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
