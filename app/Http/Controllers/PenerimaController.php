<?php

namespace App\Http\Controllers;

use App\Models\Penerima;
use App\Http\Resources\PenerimaResource;
use Illuminate\Http\Request;

class PenerimaController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/penerima",
     *     summary="List all penerima",
     *     tags={"Penerima"},
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="pekerjaan_id", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="komunal", in="query", required=false, @OA\Schema(type="boolean")),
     *     @OA\Parameter(name="search", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = Penerima::with('pekerjaan');

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        // Filter by pekerjaan_id
        if ($request->filled('pekerjaan_id')) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
        }

        // Filter by komunal only when query param is explicitly provided.
        // Note: $request->boolean() always returns bool (never null), so it must not
        // be used as the presence check — that would hide all komunal rows by default.
        if ($request->has('komunal')) {
            $query->komunal($request->boolean('komunal'));
        }

        // Search by nama
        if ($request->filled('search')) {
            $query->searchNama($request->search);
        }

        $per_page = $request->get('per_page', 20);

        if ($per_page == -1) {
            $penerima = $query->latest()->get();
            return PenerimaResource::collection($penerima);
        }

        $penerima = $query->latest()->paginate($per_page);

        return PenerimaResource::collection($penerima);
    }

    /**
     * @OA\Post(
     *     path="/api/penerima",
     *     summary="Create new penerima",
     *     tags={"Penerima"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"pekerjaan_id", "nama"},
     *             @OA\Property(property="pekerjaan_id", type="integer"),
     *             @OA\Property(property="nama", type="string"),
     *             @OA\Property(property="jumlah_jiwa", type="integer"),
     *             @OA\Property(property="nik", type="string"),
     *             @OA\Property(property="is_komunal", type="boolean")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Penerima created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|integer|exists:tbl_pekerjaan,id',
            'nama' => 'required|string|max:255',
            'jumlah_jiwa' => 'nullable|integer|min:1',
            'nik' => 'nullable|string|max:255',
            'alamat' => 'nullable|string|max:255',
            'is_komunal' => 'boolean',
        ]);

        $penerima = Penerima::create($validated);
        $penerima->load('pekerjaan');

        return new PenerimaResource($penerima);
    }

    /**
     * @OA\Get(
     *     path="/api/penerima/{id}",
     *     summary="Get penerima detail",
     *     tags={"Penerima"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Penerima $penerima)
    {
        $penerima->load('pekerjaan');
        return new PenerimaResource($penerima);
    }

    /**
     * @OA\Put(
     *     path="/api/penerima/{id}",
     *     summary="Update penerima",
     *     tags={"Penerima"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Penerima updated")
     * )
     */
    public function update(Request $request, Penerima $penerima)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|integer|exists:tbl_pekerjaan,id',
            'nama' => 'nullable|string|max:255',
            'jumlah_jiwa' => 'nullable|integer|min:1',
            'nik' => 'nullable|string|max:255',
            'alamat' => 'nullable|string|max:255',
            'is_komunal' => 'nullable|boolean',
        ]);

        $penerima->update($validated);
        $penerima->load('pekerjaan');

        return new PenerimaResource($penerima);
    }

    /**
     * @OA\Delete(
     *     path="/api/penerima/{id}",
     *     summary="Delete penerima",
     *     tags={"Penerima"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Penerima deleted")
     * )
     */
    public function destroy(Penerima $penerima)
    {
        $penerima->delete();
        return response()->json(['message' => 'Penerima berhasil dihapus'], 200);
    }

    /**
     * @OA\Get(
     *     path="/api/penerima/pekerjaan/{pekerjaanId}",
     *     summary="Get penerima by pekerjaan ID",
     *     tags={"Penerima"},
     *     @OA\Parameter(name="pekerjaanId", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function byPekerjaan($pekerjaanId)
    {
        $perPage = request()->input('per_page', 50);
        
        if ($perPage == -1) {
            $penerima = Penerima::where('pekerjaan_id', $pekerjaanId)
                ->with('pekerjaan')
                ->latest()
                ->get();
        } else {
            $penerima = Penerima::where('pekerjaan_id', $pekerjaanId)
                ->with('pekerjaan')
                ->latest()
                ->paginate($perPage);
        }

        return PenerimaResource::collection($penerima);
    }

    /**
     * @OA\Get(
     *     path="/api/penerima/pekerjaan/{pekerjaanId}/count",
     *     summary="Get komunal penerima count",
     *     tags={"Penerima"},
     *     @OA\Parameter(name="pekerjaanId", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function komunalCount($pekerjaanId)
    {
        $total = Penerima::where('pekerjaan_id', $pekerjaanId)->count();
        $komunal = Penerima::where('pekerjaan_id', $pekerjaanId)->komunal(true)->count();

        return response()->json([
            'pekerjaan_id' => $pekerjaanId,
            'total_penerima' => $total,
            'komunal_count' => $komunal,
            'non_komunal_count' => $total - $komunal,
        ]);
    }
    /**
     * Rekap penerima per tahun anggaran → bidang (kegiatan.sub_bidang) → kecamatan → desa.
     * Hanya grup yang punya penerima yang dikembalikan (tahun kosong tidak muncul).
     */
    public function rekap(Request $request)
    {
        $rows = \DB::table('tbl_penerima as p')
            ->join('tbl_pekerjaan as pk', 'pk.id', '=', 'p.pekerjaan_id')
            ->leftJoin('tbl_kegiatan as k', 'k.id', '=', 'pk.kegiatan_id')
            ->leftJoin('tbl_kecamatan as kc', 'kc.id', '=', 'pk.kecamatan_id')
            ->leftJoin('tbl_desa as d', 'd.id', '=', 'pk.desa_id')
            // Paket jasa konsultansi tidak punya desa/kecamatan — kecualikan.
            ->where(function ($q) {
                $q->where('pk.is_konsultan', false)->orWhereNull('pk.is_konsultan');
            })
            ->when($request->filled('tahun'), fn ($q) => $q->where('k.tahun_anggaran', $request->tahun))
            ->when($request->filled('bidang'), fn ($q) => $q->where('k.sub_bidang', $request->bidang))
            ->select('k.tahun_anggaran', 'k.sub_bidang', 'kc.n_kec', 'd.n_desa')
            ->selectRaw('count(*) as penerima_kk, coalesce(sum(p.jumlah_jiwa), 0) as total_jiwa')
            ->groupBy('k.tahun_anggaran', 'k.sub_bidang', 'kc.n_kec', 'd.n_desa')
            ->orderByDesc('k.tahun_anggaran')
            ->orderBy('k.sub_bidang')
            ->orderBy('kc.n_kec')
            ->orderBy('d.n_desa')
            ->get();

        // Susun pohon tahun → bidang → kecamatan → desa.
        $tahun = [];
        foreach ($rows as $row) {
            $t = $row->tahun_anggaran ?: '(Tanpa tahun)';
            $b = $row->sub_bidang ?: 'Lainnya';
            $kc = $row->n_kec ?: '(Tanpa kecamatan)';
            $ds = $row->n_desa ?: '(Tanpa desa)';

            $tahun[$t] ??= ['tahun' => $t, 'penerima_kk' => 0, 'total_jiwa' => 0, 'bidang' => []];
            $tahun[$t]['bidang'][$b] ??= ['bidang' => $b, 'penerima_kk' => 0, 'total_jiwa' => 0, 'kecamatan' => []];
            $tahun[$t]['bidang'][$b]['kecamatan'][$kc] ??= ['kecamatan' => $kc, 'penerima_kk' => 0, 'total_jiwa' => 0, 'desa' => []];
            $tahun[$t]['bidang'][$b]['kecamatan'][$kc]['desa'][$ds] = [
                'desa' => $ds,
                'penerima_kk' => (int) $row->penerima_kk,
                'total_jiwa' => (int) $row->total_jiwa,
            ];

            $tahun[$t]['penerima_kk'] += $row->penerima_kk;
            $tahun[$t]['total_jiwa'] += $row->total_jiwa;
            $tahun[$t]['bidang'][$b]['penerima_kk'] += $row->penerima_kk;
            $tahun[$t]['bidang'][$b]['total_jiwa'] += $row->total_jiwa;
            $tahun[$t]['bidang'][$b]['kecamatan'][$kc]['penerima_kk'] += $row->penerima_kk;
            $tahun[$t]['bidang'][$b]['kecamatan'][$kc]['total_jiwa'] += $row->total_jiwa;
        }

        return response()->json([
            'data' => array_values(array_map(function ($t) {
                $t['bidang'] = array_values(array_map(function ($b) {
                    $b['kecamatan'] = array_values(array_map(function ($kc) {
                        $kc['desa'] = array_values($kc['desa']);
                        return $kc;
                    }, $b['kecamatan']));
                    return $b;
                }, $t['bidang']));
                return $t;
            }, $tahun)),
        ]);
    }

    public function summary(Request $request)
    {
        $query = Penerima::query()
            // Paket jasa konsultansi tidak punya desa/kecamatan — kecualikan.
            ->whereHas('pekerjaan', function ($q) {
                $q->where('is_konsultan', false)->orWhereNull('is_konsultan');
            });

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->filled('bidang')) {
            $query->whereHas('pekerjaan.kegiatan', function ($q) use ($request) {
                $q->where('sub_bidang', $request->bidang);
            });
        }

        $total = (clone $query)->count();
        $komunal = (clone $query)->komunal(true)->count();
        $totalJiwa = (clone $query)->sum('jumlah_jiwa');

        return response()->json([
            'total_penerima' => $total,
            'komunal_count' => $komunal,
            'individu_count' => $total - $komunal,
            'total_jiwa' => $totalJiwa,
        ]);
    }
}
    