<?php

namespace App\Http\Controllers;

use App\Models\KegiatanRole;
use App\Models\Kegiatan;
use Spatie\Permission\Models\Role;
use Illuminate\Http\Request;

class KegiatanRoleController extends Controller
{
    public function index()
    {
        return KegiatanRole::with(['role', 'kegiatan'])->paginate(20);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'role_id' => 'required|exists:roles,id',
            'kegiatan_id' => 'required|exists:tbl_kegiatan,id|unique:kegiatan_role,kegiatan_id,NULL,id,role_id,' . $request->role_id,
        ]);

        $kegiatanRole = KegiatanRole::create($validated);
        return response()->json($kegiatanRole->load(['role', 'kegiatan']), 201);
    }

    public function destroy(KegiatanRole $kegiatanRole)
    {
        $kegiatanRole->delete();
        return response()->json(['message' => 'Kegiatan-role mapping deleted']);
    }
}
