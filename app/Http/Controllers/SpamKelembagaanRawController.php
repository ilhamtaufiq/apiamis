<?php

namespace App\Http\Controllers;

use App\Http\Resources\SpamKelembagaanRawResource;
use App\Models\SpamKelembagaanRaw;
use App\Services\SpamKelembagaanRawImportService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class SpamKelembagaanRawController extends Controller
{
    private function getFilteredQuery(Request $request)
    {
        $query = SpamKelembagaanRaw::query();

        if ($request->filled('search')) {
            $search = $request->string('search')->toString();
            $query->where(function ($q) use ($search) {
                $q->where('kecamatan', 'LIKE', "%{$search}%")
                    ->orWhere('desa_kelurahan', 'LIKE', "%{$search}%")
                    ->orWhere('nama_pengelola', 'LIKE', "%{$search}%")
                    ->orWhere('program_pembangunan', 'LIKE', "%{$search}%");
            });
        }

        foreach (['jenis_jaringan', 'kecamatan', 'sumber_dana_raw'] as $filter) {
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

        $allRecords = $query->get();
        $grouped = $allRecords->groupBy('desa_kelurahan_normalized');

        $desaNames = $grouped->keys()->toArray();
        // MySQL is case-insensitive, so whereIn finds the rows
        $desasRaw = \App\Models\Desa::whereIn('n_desa', $desaNames)
            ->pluck('jumlah_penduduk', 'n_desa');

        // Convert keys to uppercase to match PHP's case-sensitive lookup
        $desas = collect();
        foreach ($desasRaw as $k => $v) {
            $desas->put(trim(strtoupper($k)), $v);
        }

        $page = (int) $request->get('page', 1);
        $perPage = min((int) $request->get('per_page', 15), 100);

        $items = $grouped->map(function ($group, $desaName) use ($desas) {
            $lookupKey = trim(strtoupper($desaName));
            return [
                'kecamatan' => $group->first()->kecamatan,
                'desa_kelurahan' => $group->first()->desa_kelurahan,
                'desa_kelurahan_normalized' => $desaName,
                'jumlah_jp' => $group->where('jenis_jaringan', 'JP')->count(),
                'jumlah_bjp' => $group->where('jenis_jaringan', 'BJP')->count(),
                'total_sr' => $group->sum('sr_unit'),
                'total_kk_terlayani' => $group->sum('kk_terlayani'),
                'total_jiwa_terlayani' => $group->sum('jiwa_terlayani'),
                'target_layanan' => (int) $desas->get($lookupKey, 0),
                'sistem_list' => SpamKelembagaanRawResource::collection($group)->resolve(),
            ];
        })->values();

        $paginated = new \Illuminate\Pagination\LengthAwarePaginator(
            $items->slice(($page - 1) * $perPage, $perPage)->values(),
            $items->count(),
            $perPage,
            $page,
            ['path' => $request->url(), 'query' => $request->query()]
        );

        return response()->json([
            'success' => true,
            'data' => $paginated->items(),
            'meta' => [
                'current_page' => $paginated->currentPage(),
                'last_page' => $paginated->lastPage(),
                'per_page' => $paginated->perPage(),
                'total' => $paginated->total(),
                'from' => $paginated->firstItem(),
                'to' => $paginated->lastItem(),
            ],
        ]);
    }

    public function show(SpamKelembagaanRaw $spamKelembagaanRaw): JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => new SpamKelembagaanRawResource($spamKelembagaanRaw),
        ]);
    }

    public function stats(Request $request): JsonResponse
    {
        $baseQuery = $this->getFilteredQuery($request);

        return response()->json([
            'success' => true,
            'data' => [
                'total' => (clone $baseQuery)->count(),
                'jp' => (clone $baseQuery)->where('jenis_jaringan', 'JP')->count(),
                'bjp' => (clone $baseQuery)->where('jenis_jaringan', 'BJP')->count(),
                'kecamatan' => (clone $baseQuery)->whereNotNull('kecamatan')->distinct('kecamatan')->count('kecamatan'),
                'total_sr' => (int) (clone $baseQuery)->sum('sr_unit'),
                'total_kk_terlayani' => (int) (clone $baseQuery)->sum('kk_terlayani'),
                'total_kk_jp' => (int) (clone $baseQuery)->where('jenis_jaringan', 'JP')->sum('kk_terlayani'),
                'total_kk_bjp' => (int) (clone $baseQuery)->where('jenis_jaringan', 'BJP')->sum('kk_terlayani'),
                'total_jiwa_terlayani' => (int) (clone $baseQuery)->sum('jiwa_terlayani'),
                'total_target_layanan' => (int) \App\Models\Desa::whereIn(
                    'n_desa',
                    (clone $baseQuery)->whereNotNull('desa_kelurahan_normalized')->distinct()->pluck('desa_kelurahan_normalized')
                )->sum('jumlah_penduduk'),
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

    public function options(): JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => [
                'kecamatan' => SpamKelembagaanRaw::query()
                    ->whereNotNull('kecamatan')
                    ->distinct()
                    ->orderBy('kecamatan')
                    ->pluck('kecamatan')
                    ->values(),
                'sumber_dana' => SpamKelembagaanRaw::query()
                    ->whereNotNull('sumber_dana_raw')
                    ->distinct()
                    ->orderBy('sumber_dana_raw')
                    ->pluck('sumber_dana_raw')
                    ->values(),
            ],
        ]);
    }

    public function import(Request $request, SpamKelembagaanRawImportService $importer): JsonResponse
    {
        $validated = $request->validate([
            'file' => ['required', 'file', 'mimes:xlsx,xls', 'max:20480'],
            'replace' => ['nullable', 'boolean'],
        ]);

        $result = $importer->import($request->file('file'), (bool) ($validated['replace'] ?? false));

        return response()->json([
            'success' => true,
            'message' => 'Data kelembagaan SPAM berhasil diimpor.',
            'data' => $result,
        ], 201);
    }
}
