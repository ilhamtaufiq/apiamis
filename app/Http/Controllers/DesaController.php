<?php

namespace App\Http\Controllers;

use App\Models\Desa;
use App\Http\Resources\DesaResource;
use App\Services\DesaKkSyncService;
use Illuminate\Http\Request;
use RuntimeException;
use Throwable;

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
        $kecamatanId = $request->query('kecamatan_id');

        $query = Desa::with('kecamatan');

        if ($kecamatanId) {
            $query->where('kecamatan_id', $kecamatanId);
        }

        if ($search) {
            $query->where(function ($q) use ($search) {
                $q->where('n_desa', 'like', "%{$search}%")
                    ->orWhereHas('kecamatan', function ($kec) use ($search) {
                        $kec->where('n_kec', 'like', "%{$search}%");
                    });
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
            'jumlah_kk' => 'nullable|integer|min:0',
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
            'jumlah_kk' => 'nullable|integer|min:0',
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

    /**
     * @OA\Post(
     *     path="/api/desa/sync-kk",
     *     summary="Sync jumlah KK per desa dari open data Cianjur",
     *     tags={"Desa"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=false,
     *         @OA\JsonContent(
     *             @OA\Property(property="tahun", type="integer", example=2025),
     *             @OA\Property(property="semester", type="integer", example=2)
     *         )
     *     ),
     *     @OA\Response(response=200, description="Sync completed")
     * )
     */
    public function syncKk(Request $request, DesaKkSyncService $service)
    {
        $validated = $request->validate([
            'tahun' => 'nullable|integer|min:2000|max:2100',
            'semester' => 'nullable|integer|in:1,2',
        ]);

        try {
            $result = $service->sync(
                isset($validated['tahun']) ? (int) $validated['tahun'] : null,
                isset($validated['semester']) ? (int) $validated['semester'] : null,
            );
        } catch (RuntimeException $e) {
            return response()->json([
                'message' => $e->getMessage(),
            ], 422);
        } catch (Throwable $e) {
            report($e);

            return response()->json([
                'message' => 'Gagal sinkronisasi jumlah KK.',
            ], 500);
        }

        return response()->json([
            'message' => sprintf(
                'Sync KK selesai: %d desa diperbarui (tahun %s semester %s).',
                $result['updated'],
                $result['tahun'] ?? '-',
                $result['semester'] ?? '-'
            ),
            'data' => $result,
        ]);
    }
}
