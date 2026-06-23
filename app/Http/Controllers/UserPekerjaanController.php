<?php

namespace App\Http\Controllers;

use App\Models\Pekerjaan;
use App\Models\User;
use App\Notifications\AppNotification;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class UserPekerjaanController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/user-pekerjaan",
     *     summary="List all user-pekerjaan assignments",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index()
    {
        $assignments = DB::table('user_pekerjaan')
            ->join('users', 'user_pekerjaan.user_id', '=', 'users.id')
            ->join('tbl_pekerjaan', 'user_pekerjaan.pekerjaan_id', '=', 'tbl_pekerjaan.id')
            ->select(
                'user_pekerjaan.id',
                'user_pekerjaan.user_id',
                'user_pekerjaan.pekerjaan_id',
                'users.name as user_name',
                'users.email as user_email',
                'tbl_pekerjaan.nama_paket as pekerjaan_nama',
                'tbl_pekerjaan.pagu as pekerjaan_pagu',
                'user_pekerjaan.created_at'
            )
            ->orderBy('user_pekerjaan.created_at', 'desc')
            ->get();

        return response()->json([
            'status' => 'success',
            'data' => $assignments
        ]);
    }

    /**
     * @OA\Post(
     *     path="/api/user-pekerjaan",
     *     summary="Assign user to multiple pekerjaan",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"user_id", "pekerjaan_ids"},
     *             @OA\Property(property="user_id", type="integer"),
     *             @OA\Property(property="pekerjaan_ids", type="array", @OA\Items(type="integer"))
     *         )
     *     ),
     *     @OA\Response(response=201, description="Assigned")
     * )
     */
    public function store(Request $request)
    {
        $request->validate([
            'user_id' => 'required|exists:users,id',
            'pekerjaan_ids' => 'required|array',
            'pekerjaan_ids.*' => 'exists:tbl_pekerjaan,id'
        ]);

        $user = User::findOrFail($request->user_id);
        
        // Sync pekerjaan (this will add new ones and remove unselected)
        $user->assignedPekerjaan()->syncWithoutDetaching($request->pekerjaan_ids);

        // Automatically grant the 'pengawas' role if not already present.
        // The Puspen /pengawas-kpi feature only shows users who have this role + assignments.
        if (!$user->hasRole('pengawas')) {
            $user->assignRole('pengawas');
        }

        // Notify User
        $pekerjaanNames = Pekerjaan::whereIn('id', $request->pekerjaan_ids)->pluck('nama_paket')->toArray();
        $user->notify(new AppNotification(
            'Penugasan Pekerjaan Baru',
            'Anda telah di-assign ke ' . count($request->pekerjaan_ids) . ' pekerjaan baru: ' . implode(', ', $pekerjaanNames),
            '/pekerjaan',
            'info'
        ));

        return response()->json([
            'status' => 'success',
            'message' => 'Pekerjaan berhasil di-assign ke user'
        ], 201);
    }

    /**
     * @OA\Delete(
     *     path="/api/user-pekerjaan/{id}",
     *     summary="Remove assignment",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy($id)
    {
        $deleted = DB::table('user_pekerjaan')->where('id', $id)->delete();

        if (!$deleted) {
            return response()->json([
                'status' => 'error',
                'message' => 'Assignment tidak ditemukan'
            ], 404);
        }

        return response()->json([
            'status' => 'success',
            'message' => 'Assignment berhasil dihapus'
        ]);
    }

    /**
     * @OA\Get(
     *     path="/api/user-pekerjaan/user/{userId}",
     *     summary="Get assignments by user",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     @OA\Parameter(name="userId", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function byUser($userId)
    {
        $user = User::with('assignedPekerjaan.kecamatan', 'assignedPekerjaan.desa', 'assignedPekerjaan.kegiatan')
            ->findOrFail($userId);

        return response()->json([
            'status' => 'success',
            'data' => [
                'user' => [
                    'id' => $user->id,
                    'name' => $user->name,
                    'email' => $user->email
                ],
                'pekerjaan' => $user->assignedPekerjaan
            ]
        ]);
    }

    /**
     * @OA\Get(
     *     path="/api/user-pekerjaan/pekerjaan/{pekerjaanId}",
     *     summary="Get assignments by pekerjaan",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     @OA\Parameter(name="pekerjaanId", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function byPekerjaan($pekerjaanId)
    {
        $pekerjaan = Pekerjaan::with('assignedUsers')
            ->findOrFail($pekerjaanId);

        return response()->json([
            'status' => 'success',
            'data' => [
                'pekerjaan' => [
                    'id' => $pekerjaan->id,
                    'nama_paket' => $pekerjaan->nama_paket,
                    'pagu' => $pekerjaan->pagu
                ],
                'users' => $pekerjaan->assignedUsers
            ]
        ]);
    }

    /**
     * @OA\Get(
     *     path="/api/user-pekerjaan/available-users",
     *     summary="Get users available for assignment (non-admin)",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function availableUsers()
    {
        $users = User::whereDoesntHave('roles', function ($query) {
                $query->where('name', 'admin');
            })
            ->select('id', 'name', 'email')
            ->orderBy('name')
            ->get();

        return response()->json([
            'status' => 'success',
            'data' => $users
        ]);
    }
}
