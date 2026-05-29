<?php

namespace App\Http\Controllers;

use App\Http\Resources\KegiatanResource;
use App\Models\Kegiatan;
use Illuminate\Http\Request;
use Illuminate\Validation\Rule;

class KegiatanController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/kegiatan",
     *     summary="List all kegiatan",
     *     tags={"Kegiatan"},
     *
     *     @OA\Parameter(
     *         name="tahun",
     *         in="query",
     *         description="Filter by year",
     *         required=false,
     *
     *         @OA\Schema(type="string")
     *     ),
     *
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = Kegiatan::query();

        if ($request->has('tahun') && $request->tahun) {
            $query->where('tahun_anggaran', $request->tahun);
        }

        if ($request->has('per_page') && $request->per_page == -1) {
            return KegiatanResource::collection($query->get());
        }

        $kegiatan = $query->paginate($request->get('per_page', 15));

        return KegiatanResource::collection($kegiatan);
    }

    /**
     * @OA\Post(
     *     path="/api/kegiatan",
     *     summary="Create new kegiatan",
     *     tags={"Kegiatan"},
     *     security={{"bearerAuth":{}}},
     *
     *     @OA\RequestBody(
     *         required=true,
     *
     *         @OA\JsonContent(
     *
     *             @OA\Property(property="nama_program", type="string"),
     *             @OA\Property(property="nama_kegiatan", type="string"),
     *             @OA\Property(property="tahun_anggaran", type="string", example="2024"),
     *             @OA\Property(property="pagu", type="number")
     *         )
     *     ),
     *
     *     @OA\Response(response=201, description="Kegiatan created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'nama_program' => 'nullable|string|max:255',
            'sub_bidang' => 'nullable|string|max:255',
            'nama_kegiatan' => 'nullable|string|max:255',
            'nama_sub_kegiatan' => 'nullable|string|max:255',
            'tahun_anggaran' => 'nullable|string|max:50',
            'sumber_dana' => ['nullable', 'string', Rule::in(Kegiatan::SUMBER_DANA_OPTIONS)],
            'pagu' => 'nullable|numeric|min:0',
            'kode_rekening' => 'nullable|array',
        ]);

        $kegiatan = Kegiatan::create($validated);

        return new KegiatanResource($kegiatan);
    }

    /**
     * @OA\Get(
     *     path="/api/kegiatan/{id}",
     *     summary="Get kegiatan detail",
     *     tags={"Kegiatan"},
     *
     *     @OA\Parameter(
     *         name="id",
     *         in="path",
     *         required=true,
     *
     *         @OA\Schema(type="integer")
     *     ),
     *
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Kegiatan $kegiatan)
    {
        return new KegiatanResource($kegiatan);
    }

    /**
     * @OA\Put(
     *     path="/api/kegiatan/{id}",
     *     summary="Update kegiatan",
     *     tags={"Kegiatan"},
     *     security={{"bearerAuth":{}}},
     *
     *     @OA\Parameter(
     *         name="id",
     *         in="path",
     *         required=true,
     *
     *         @OA\Schema(type="integer")
     *     ),
     *
     *     @OA\RequestBody(
     *         required=true,
     *
     *         @OA\JsonContent(
     *
     *             @OA\Property(property="nama_program", type="string")
     *         )
     *     ),
     *
     *     @OA\Response(response=200, description="Kegiatan updated")
     * )
     */
    public function update(Request $request, Kegiatan $kegiatan)
    {
        $validated = $request->validate([
            'nama_program' => 'nullable|string|max:255',
            'sub_bidang' => 'nullable|string|max:255',
            'nama_kegiatan' => 'nullable|string|max:255',
            'nama_sub_kegiatan' => 'nullable|string|max:255',
            'tahun_anggaran' => 'nullable|string|max:50',
            'sumber_dana' => ['nullable', 'string', Rule::in(Kegiatan::SUMBER_DANA_OPTIONS)],
            'pagu' => 'nullable|numeric|min:0',
            'kode_rekening' => 'nullable|array',
        ]);

        $kegiatan->update($validated);

        return new KegiatanResource($kegiatan);
    }

    /**
     * @OA\Delete(
     *     path="/api/kegiatan/{id}",
     *     summary="Delete kegiatan",
     *     tags={"Kegiatan"},
     *     security={{"bearerAuth":{}}},
     *
     *     @OA\Parameter(
     *         name="id",
     *         in="path",
     *         required=true,
     *
     *         @OA\Schema(type="integer")
     *     ),
     *
     *     @OA\Response(response=200, description="Kegiatan deleted")
     * )
     */
    public function destroy(Kegiatan $kegiatan)
    {
        $kegiatan->delete();

        return response()->json(['message' => 'Kegiatan deleted successfully'], 200);
    }

    /**
     * @OA\Get(
     *     path="/api/kegiatan/tahun/{tahun}",
     *     summary="Filter kegiatan by year",
     *     tags={"Kegiatan"},
     *
     *     @OA\Parameter(
     *         name="tahun",
     *         in="path",
     *         required=true,
     *
     *         @OA\Schema(type="string")
     *     ),
     *
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function byTahun($tahun)
    {
        $kegiatan = Kegiatan::where('tahun_anggaran', $tahun)->paginate(15);

        return KegiatanResource::collection($kegiatan);
    }
}
