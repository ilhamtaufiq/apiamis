<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;

class MasterFasePekerjaanController extends Controller
{
    public function index(Request $request)
    {
        $query = \App\Models\MasterFasePekerjaan::query();

        if ($request->has('jenis_proyek')) {
            $query->where('jenis_proyek', $request->jenis_proyek);
        }

        $fases = $query->orderBy('jenis_proyek')
                       ->orderBy('prioritas')
                       ->get();

        return response()->json([
            'success' => true,
            'data' => $fases
        ]);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'jenis_proyek' => 'required|string|max:50',
            'kode_fase' => 'required|string|max:30',
            'nama_fase' => 'required|string|max:100',
            'prioritas' => 'required|integer',
            'overlap_persen' => 'integer|min:0|max:100',
            'durasi_faktor' => 'numeric|min:0.1',
            'keywords' => 'required|array',
            'deskripsi' => 'nullable|string',
            'is_active' => 'boolean'
        ]);

        $fase = \App\Models\MasterFasePekerjaan::create($validated);

        return response()->json([
            'success' => true,
            'data' => $fase
        ], 201);
    }

    public function show($id)
    {
        $fase = \App\Models\MasterFasePekerjaan::findOrFail($id);
        
        return response()->json([
            'success' => true,
            'data' => $fase
        ]);
    }

    public function update(Request $request, $id)
    {
        $fase = \App\Models\MasterFasePekerjaan::findOrFail($id);

        $validated = $request->validate([
            'jenis_proyek' => 'string|max:50',
            'kode_fase' => 'string|max:30',
            'nama_fase' => 'string|max:100',
            'prioritas' => 'integer',
            'overlap_persen' => 'integer|min:0|max:100',
            'durasi_faktor' => 'numeric|min:0.1',
            'keywords' => 'array',
            'deskripsi' => 'nullable|string',
            'is_active' => 'boolean'
        ]);

        $fase->update($validated);

        return response()->json([
            'success' => true,
            'data' => $fase
        ]);
    }

    public function destroy($id)
    {
        $fase = \App\Models\MasterFasePekerjaan::findOrFail($id);
        $fase->delete();

        return response()->json([
            'success' => true,
            'message' => 'Data deleted successfully'
        ]);
    }
}
