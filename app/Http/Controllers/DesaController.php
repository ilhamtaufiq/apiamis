<?php

namespace App\Http\Controllers;

use App\Models\Desa;
use App\Http\Resources\DesaResource;
use Illuminate\Http\Request;

class DesaController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/desa",
     *     summary="List all desa",
     *     tags={"Desa"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $perPage = $request->query('per_page', 15);
        $search = $request->query('search');

        $query = Desa::with('kecamatan');

        if ($search) {
            $query->where('n_desa', 'like', "%{$search}%")
                  ->orWhereHas('kecamatan', function($q) use ($search) {
                      $q->where('n_kec', 'like', "%{$search}%");
                  });
        }

        $desa = $query->paginate($perPage);
        return DesaResource::collection($desa);
    }

    /**
     * @OA\Post(
     *     path="/api/desa",
     *     summary="Create new desa",
     *     tags={"Desa"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"nama_desa","kecamatan_id"},
     *             @OA\Property(property="nama_desa", type="string", example="Argapura"),
     *             @OA\Property(property="kecamatan_id", type="integer", example=1)
     *         )
     *     ),
     *     @OA\Response(response=201, description="Desa created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'nama_desa' => 'required|string|max:100',
            'luas' => 'required|numeric',
            'jumlah_penduduk' => 'required|integer',
            'kecamatan_id' => 'required|exists:tbl_kecamatan,id'
        ]);

        // Map nama_desa to n_desa for database
        $data = $validated;
        $data['n_desa'] = $data['nama_desa'];
        unset($data['nama_desa']);

        $desa = Desa::create($data);
        return new DesaResource($desa->load('kecamatan'));
    }

    /**
     * @OA\Get(
     *     path="/api/desa/{id}",
     *     summary="Get desa detail",
     *     tags={"Desa"},
     *     @OA\Parameter(
     *         name="id",
     *         in="path",
     *         required=true,
     *         @OA\Schema(type="integer")
     *     ),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Desa $desa)
    {
        $desa->load('kecamatan');
        return new DesaResource($desa);
    }

    /**
     * @OA\Put(
     *     path="/api/desa/{id}",
     *     summary="Update desa",
     *     tags={"Desa"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(
     *         name="id",
     *         in="path",
     *         required=true,
     *         @OA\Schema(type="integer")
     *     ),
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             @OA\Property(property="nama_desa", type="string")
     *         )
     *     ),
     *     @OA\Response(response=200, description="Desa updated")
     * )
     */
    public function update(Request $request, Desa $desa)
    {
        $validated = $request->validate([
            'nama_desa' => 'nullable|string|max:100',
            'luas' => 'nullable|numeric',
            'jumlah_penduduk' => 'nullable|integer',
            'kecamatan_id' => 'nullable|exists:tbl_kecamatan,id'
        ]);

        // Map nama_desa to n_desa for database
        $data = $validated;
        if (isset($data['nama_desa'])) {
            $data['n_desa'] = $data['nama_desa'];
            unset($data['nama_desa']);
        }

        $desa->update($data);
        return new DesaResource($desa->load('kecamatan'));
    }

    /**
     * @OA\Delete(
     *     path="/api/desa/{id}",
     *     summary="Delete desa",
     *     tags={"Desa"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(
     *         name="id",
     *         in="path",
     *         required=true,
     *         @OA\Schema(type="integer")
     *     ),
     *     @OA\Response(response=200, description="Desa deleted")
     * )
     */
    public function destroy(Desa $desa)
    {
        $desa->delete();
        return response()->json(['message' => 'Desa deleted successfully'], 200);
    }

    /**
     * @OA\Get(
     *     path="/api/desa/kecamatan/{kecamatanId}",
     *     summary="Get desa by kecamatan ID",
     *     tags={"Desa"},
     *     @OA\Parameter(
     *         name="kecamatanId",
     *         in="path",
     *         required=true,
     *         @OA\Schema(type="integer")
     *     ),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function byKecamatan($kecamatanId)
    {
        $desa = Desa::where('kecamatan_id', $kecamatanId)->get();
        return DesaResource::collection($desa);
    }
}
