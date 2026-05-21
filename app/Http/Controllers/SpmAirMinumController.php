<?php

namespace App\Http\Controllers;

use App\Http\Resources\SpmAirMinumResource;
use App\Models\SpamWilayahMatch;
use App\Models\SpmAirMinum;
use App\Services\SpmAirMinumConsolidationService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

class SpmAirMinumController extends Controller
{
    public function index(Request $request): JsonResponse
    {
        $query = SpmAirMinum::with(['kecamatan', 'desa'])->orderBy('kecamatan_id')->orderBy('desa_id');
        $jenisJaringan = $this->jenisJaringan($request);

        if ($request->filled('search')) {
            $search = $request->string('search')->toString();
            $query->where(function ($q) use ($search) {
                $q->whereHas('desa', fn ($desa) => $desa->where('n_desa', 'LIKE', "%{$search}%"))
                    ->orWhereHas('kecamatan', fn ($kecamatan) => $kecamatan->where('n_kec', 'LIKE', "%{$search}%"));
            });
        }

        if ($request->filled('kecamatan_id')) {
            $query->where('kecamatan_id', (int) $request->get('kecamatan_id'));
        }

        if ($request->filled('status_spm')) {
            $this->applyStatusFilter($query, $request->string('status_spm')->toString(), $jenisJaringan);
        }

        $perPage = min((int) $request->get('per_page', 15), 100);
        $records = $query->paginate($perPage);

        return response()->json([
            'success' => true,
            'data' => SpmAirMinumResource::collection($records)->resolve($request),
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

    public function show(SpmAirMinum $spmAirMinum): JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => new SpmAirMinumResource($spmAirMinum->load(['kecamatan', 'desa', 'sources'])),
        ]);
    }

    public function stats(Request $request): JsonResponse
    {
        $jenisJaringan = $this->jenisJaringan($request);
        $layananColumn = $this->layananColumn($jenisJaringan);

        return response()->json([
            'success' => true,
            'data' => [
                'total_desa' => SpmAirMinum::count(),
                'terpenuhi' => $this->statusCount('terpenuhi', $jenisJaringan),
                'belum_terpenuhi' => $this->statusCount('belum_terpenuhi', $jenisJaringan),
                'data_kurang' => $this->statusCount('data_kurang', $jenisJaringan),
                'target_total_jiwa' => (int) SpmAirMinum::sum('target_total_jiwa'),
                'jp_jiwa_terlayani' => $jenisJaringan === 'BJP' ? 0 : (int) SpmAirMinum::sum('jp_jiwa_terlayani'),
                'bjp_jiwa_terlayani' => $jenisJaringan === 'JP' ? 0 : (int) SpmAirMinum::sum('bjp_jiwa_terlayani'),
                'total_jiwa_terlayani' => (int) SpmAirMinum::sum($layananColumn),
                'match' => [
                    'matched' => SpamWilayahMatch::where('match_status', 'matched')->count(),
                    'ambiguous' => SpamWilayahMatch::where('match_status', 'ambiguous')->count(),
                    'unmatched' => SpamWilayahMatch::where('match_status', 'unmatched')->count(),
                ],
            ],
        ]);
    }

    public function unmatched(Request $request): JsonResponse
    {
        $query = SpamWilayahMatch::query()
            ->whereIn('match_status', ['unmatched', 'ambiguous'])
            ->latest('id');

        if ($request->filled('source_type')) {
            $query->where('source_type', $request->string('source_type')->toString());
        }

        $records = $query->paginate(min((int) $request->get('per_page', 20), 100));

        return response()->json([
            'success' => true,
            'data' => $records->items(),
            'meta' => [
                'current_page' => $records->currentPage(),
                'last_page' => $records->lastPage(),
                'per_page' => $records->perPage(),
                'total' => $records->total(),
            ],
        ]);
    }

    public function consolidate(SpmAirMinumConsolidationService $service): JsonResponse
    {
        return response()->json([
            'success' => true,
            'message' => 'Konsolidasi SPM Air Minum selesai.',
            'data' => $service->consolidate(),
        ]);
    }

    public function options(): JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => [
                'kecamatan' => SpmAirMinum::query()
                    ->join('tbl_kecamatan', 'spm_air_minum.kecamatan_id', '=', 'tbl_kecamatan.id')
                    ->select('tbl_kecamatan.id', 'tbl_kecamatan.n_kec')
                    ->distinct()
                    ->orderBy('tbl_kecamatan.n_kec')
                    ->get()
                    ->map(fn ($item) => ['id' => $item->id, 'nama' => $item->n_kec]),
                'status' => ['terpenuhi', 'belum_terpenuhi', 'data_kurang'],
                'jenis_jaringan' => ['JP', 'BJP'],
            ],
        ]);
    }

    private function jenisJaringan(Request $request): ?string
    {
        $value = strtoupper($request->string('jenis_jaringan')->toString());

        return in_array($value, ['JP', 'BJP'], true) ? $value : null;
    }

    private function layananColumn(?string $jenisJaringan): string
    {
        return match ($jenisJaringan) {
            'JP' => 'jp_jiwa_terlayani',
            'BJP' => 'bjp_jiwa_terlayani',
            default => 'total_jiwa_terlayani',
        };
    }

    private function applyStatusFilter($query, string $status, ?string $jenisJaringan): void
    {
        $layananColumn = $this->layananColumn($jenisJaringan);

        match ($status) {
            'terpenuhi' => $query->where('target_total_jiwa', '>', 0)->whereColumn($layananColumn, '>=', 'target_total_jiwa'),
            'belum_terpenuhi' => $query->where('target_total_jiwa', '>', 0)->whereColumn($layananColumn, '<', 'target_total_jiwa'),
            'data_kurang' => $query->where(fn ($q) => $q->whereNull('target_total_jiwa')->orWhere('target_total_jiwa', '<=', 0)),
            default => null,
        };
    }

    private function statusCount(string $status, ?string $jenisJaringan): int
    {
        $query = SpmAirMinum::query();
        $this->applyStatusFilter($query, $status, $jenisJaringan);

        return $query->count();
    }
}
