<?php

namespace App\Http\Controllers;

use App\Models\Pekerjaan;
use App\Models\User;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class UserPekerjaanController extends Controller
{
    /**
     * List semua assignments
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
     * Assign user ke pekerjaan
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

        return response()->json([
            'status' => 'success',
            'message' => 'Pekerjaan berhasil di-assign ke user'
        ], 201);
    }

    /**
     * Remove assignment
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
     * Get pekerjaan assigned ke user tertentu
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
     * Get users assigned ke pekerjaan tertentu
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
     * Get list users yang bisa di-assign (non-admin)
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
