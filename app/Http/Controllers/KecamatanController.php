<?php

namespace App\Http\Controllers;

use App\Models\Kecamatan;
use App\Http\Resources\KecamatanResource;
use App\Http\Resources\KecamatanDetailResource;
use Illuminate\Http\Request;

class KecamatanController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/kecamatan",
     *     summary="List all kecamatan",
     *     tags={"Kecamatan"},
     *     @OA\Response(
     *         response=200,
     *         description="Successful operation"
     *     )
     * )
     */
    public function index()
    {
        $kecamatan = Kecamatan::all();
        return KecamatanResource::collection($kecamatan);
    }

    /**
     * @OA\Post(
     *     path="/api/kecamatan",
     *     summary="Create new kecamatan",
     *     tags={"Kecamatan"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"n_kec"},
     *             @OA\Property(property="n_kec", type="string", example="Cianjur")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Kecamatan created"),
     *     @OA\Response(response=422, description="Validation error")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'n_kec' => 'required|string|max:255|unique:tbl_kecamatan,n_kec'
        ]);

        $kecamatan = Kecamatan::create($validated);
        return new KecamatanResource($kecamatan);
    }

    /**
     * @OA\Get(
     *     path="/api/kecamatan/{id}",
     *     summary="Get kecamatan detail",
     *     tags={"Kecamatan"},
     *     @OA\Parameter(
     *         name="id",
     *         in="path",
     *         required=true,
     *         @OA\Schema(type="integer")
     *     ),
     *     @OA\Response(response=200, description="Successful operation"),
     *     @OA\Response(response=404, description="Kecamatan not found")
     * )
     */
    public function show(Kecamatan $kecamatan)
    {
        $kecamatan->load('desa');
        return new KecamatanDetailResource($kecamatan);
    }

    /**
     * @OA\Put(
     *     path="/api/kecamatan/{id}",
     *     summary="Update kecamatan",
     *     tags={"Kecamatan"},
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
     *             @OA\Property(property="n_kec", type="string", example="Cianjur Town")
     *         )
     *     ),
     *     @OA\Response(response=200, description="Kecamatan updated"),
     *     @OA\Response(response=404, description="Kecamatan not found")
     * )
     */
    public function update(Request $request, Kecamatan $kecamatan)
    {
        $validated = $request->validate([
            'n_kec' => 'nullable|string|max:255|unique:tbl_kecamatan,n_kec,' . $kecamatan->id
        ]);

        $kecamatan->update($validated);
        return new KecamatanResource($kecamatan);
    }

    /**
     * @OA\Delete(
     *     path="/api/kecamatan/{id}",
     *     summary="Delete kecamatan",
     *     tags={"Kecamatan"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(
     *         name="id",
     *         in="path",
     *         required=true,
     *         @OA\Schema(type="integer")
     *     ),
     *     @OA\Response(response=200, description="Kecamatan deleted"),
     *     @OA\Response(response=404, description="Kecamatan not found")
     * )
     */
    public function destroy(Kecamatan $kecamatan)
    {
        $kecamatan->delete();
        return response()->json(['message' => 'Kecamatan deleted successfully'], 200);
    }
}
