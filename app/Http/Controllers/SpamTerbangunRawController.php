<?php

namespace App\Http\Controllers;

use App\Http\Resources\SpamTerbangunRawResource;
use App\Models\SpamTerbangunRaw;
use App\Services\SpamTerbangunRawImportService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class SpamTerbangunRawController extends Controller
{
    private function getFilteredQuery(Request $request)
    {
        $query = SpamTerbangunRaw::query();

        if ($request->filled('search')) {
            $search = $request->string('search')->toString();
            $query->where(function ($q) use ($search) {
                $q->where('kecamatan', 'LIKE', "%{$search}%")
                    ->orWhere('desa_kelurahan', 'LIKE', "%{$search}%")
                    ->orWhere('nama_pengelola', 'LIKE', "%{$search}%")
                    ->orWhere('sumber_air_baku', 'LIKE', "%{$search}%")
                    ->orWhere('asal_proyek', 'LIKE', "%{$search}%")
                    ->orWhere('keterangan', 'LIKE', "%{$search}%");
            });
        }

        foreach (['kecamatan', 'sumber_dana_raw', 'kondisi_normalized'] as $filter) {
            if ($request->filled($filter)) {
                $query->where($filter, $request->string($filter)->toString());
            }
        }

        if ($request->filled('tahun')) {
            $year = (int) $request->get('tahun');
            $query->where(function ($q) use ($year) {
                $q->where('tahun_pembangunan_awal', $year)
                    ->orWhere('tahun_pembangunan_akhir', $year);
            });
        }

        return $query;
    }

    public function index(Request $request): JsonResponse
    {
        $query = $this->getFilteredQuery($request)->latest('id');

        $perPage = min((int) $request->get('per_page', 15), 100);
        $records = $query->paginate($perPage);

        return response()->json([
            'success' => true,
            'data' => SpamTerbangunRawResource::collection($records)->resolve(),
            'meta' => [
                'current_page' => $records->currentPage(),
                'last_page' => $records->lastPage(),
                'per_page' => $records->perPage(),
                'total' => $records->total(),
                'from' => $records->firstItem(),
                'to' => $records->lastItem(),
            ],
        ]);
    }

    public function show(SpamTerbangunRaw $spamTerbangunRaw): JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => new SpamTerbangunRawResource($spamTerbangunRaw),
        ]);
    }

    public function stats(Request $request): JsonResponse
    {
        $baseQuery = $this->getFilteredQuery($request);

        return response()->json([
            'success' => true,
            'data' => [
                'total' => (clone $baseQuery)->count(),
                'kecamatan' => (clone $baseQuery)->whereNotNull('kecamatan')->distinct('kecamatan')->count('kecamatan'),
                'berfungsi' => (clone $baseQuery)->where('kondisi_normalized', 'berfungsi')->count(),
                'tidak_berfungsi' => (clone $baseQuery)->where('kondisi_normalized', 'tidak_berfungsi')->count(),
                'total_sr' => (int) (clone $baseQuery)->sum('sr_unit'),
                'total_penduduk_terlayani' => (int) (clone $baseQuery)->sum('penduduk_terlayani'),
                'total_target_layanan' => (int) (clone $baseQuery)->sum('jumlah_penduduk'),
                'sumber_dana' => (clone $baseQuery)
                    ->select('sumber_dana_raw', DB::raw('COUNT(*) as total'))
                    ->whereNotNull('sumber_dana_raw')
                    ->groupBy('sumber_dana_raw')
                    ->orderByDesc('total')
                    ->limit(12)
                    ->get(),
            ],
        ]);
    }

    public function import(Request $request, SpamTerbangunRawImportService $importer): JsonResponse
    {
        $validated = $request->validate([
            'file' => ['required', 'file', 'mimes:xlsx,xls', 'max:20480'],
            'replace' => ['nullable', 'boolean'],
        ]);

        $file = $request->file('file');
        $result = $importer->import($file, (bool) ($validated['replace'] ?? false));

        return response()->json([
            'success' => true,
            'message' => 'Data SPAM terbangun berhasil diimpor.',
            'data' => $result,
        ], 201);
    }
}
