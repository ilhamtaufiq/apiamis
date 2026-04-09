<?php

namespace App\Http\Controllers;

use App\Models\KegiatanRole;
use App\Models\Kegiatan;
use Spatie\Permission\Models\Role;
use Illuminate\Http\Request;

class KegiatanRoleController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/kegiatan-role",
     *     summary="List all kegiatan-role mappings",
     *     tags={"Access Control (Kegiatan-Role)"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index()
    {
        return KegiatanRole::with(['role', 'kegiatan'])->paginate(20);
    }

    /**
     * @OA\Post(
     *     path="/api/kegiatan-role",
     *     summary="Map a role to a kegiatan",
     *     tags={"Access Control (Kegiatan-Role)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"role_id", "kegiatan_id"},
     *             @OA\Property(property="role_id", type="integer"),
     *             @OA\Property(property="kegiatan_id", type="integer")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Mapped")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'role_id' => 'required|exists:roles,id',
            'kegiatan_id' => 'required|exists:tbl_kegiatan,id|unique:kegiatan_role,kegiatan_id,NULL,id,role_id,' . $request->role_id,
        ]);

        $kegiatanRole = KegiatanRole::create($validated);
        return response()->json($kegiatanRole->load(['role', 'kegiatan']), 201);
    }

    /**
     * @OA\Delete(
     *     path="/api/kegiatan-role/{id}",
     *     summary="Delete mapping",
     *     tags={"Access Control (Kegiatan-Role)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(KegiatanRole $kegiatanRole)
    {
        $kegiatanRole->delete();
        return response()->json(['message' => 'Kegiatan-role mapping deleted']);
    }
}
