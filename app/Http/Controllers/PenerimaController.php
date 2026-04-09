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

        // Filter by komunal
        if ($request->boolean('komunal') !== null) {
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
}
    