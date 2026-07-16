<?php

namespace App\Http\Controllers;

use App\Models\MasterFasePekerjaan;
use Illuminate\Http\Request;
use Illuminate\Validation\Rule;

class MasterFasePekerjaanController extends Controller
{
    public function index(Request $request)
    {
        $query = MasterFasePekerjaan::query();

        if ($request->filled('jenis_proyek')) {
            $query->where('jenis_proyek', $request->jenis_proyek);
        }

        if ($request->has('is_active')) {
            $active = filter_var($request->is_active, FILTER_VALIDATE_BOOLEAN, FILTER_NULL_ON_FAILURE);
            if ($active !== null) {
                $query->where('is_active', $active);
            } elseif ($request->is_active === '1' || $request->is_active === 1) {
                $query->where('is_active', true);
            } elseif ($request->is_active === '0' || $request->is_active === 0) {
                $query->where('is_active', false);
            }
        }

        $fases = $query->orderBy('jenis_proyek')
            ->orderBy('prioritas')
            ->get();

        return response()->json([
            'success' => true,
            'data' => $fases,
        ]);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'jenis_proyek' => 'required|string|max:50',
            'kode_fase' => [
                'required',
                'string',
                'max:30',
                Rule::unique('master_fase_pekerjaans', 'kode_fase')->where(
                    fn ($q) => $q->where('jenis_proyek', $request->input('jenis_proyek'))
                ),
            ],
            'nama_fase' => 'required|string|max:100',
            'prioritas' => 'required|integer|min:0',
            'overlap_persen' => 'nullable|integer|min:0|max:100',
            'durasi_faktor' => 'nullable|numeric|min:0.1',
            'keywords' => 'nullable|array',
            'keywords.*' => 'string|max:100',
            'deskripsi' => 'nullable|string',
            'is_active' => 'nullable|boolean',
        ]);

        $validated['keywords'] = array_values($validated['keywords'] ?? []);
        $validated['overlap_persen'] = $validated['overlap_persen'] ?? 0;
        $validated['durasi_faktor'] = $validated['durasi_faktor'] ?? 1.0;
        $validated['is_active'] = $validated['is_active'] ?? true;

        $fase = MasterFasePekerjaan::create($validated);

        return response()->json([
            'success' => true,
            'data' => $fase,
        ], 201);
    }

    public function show($id)
    {
        $fase = MasterFasePekerjaan::findOrFail($id);

        return response()->json([
            'success' => true,
            'data' => $fase,
        ]);
    }

    public function update(Request $request, $id)
    {
        $fase = MasterFasePekerjaan::findOrFail($id);

        $validated = $request->validate([
            'jenis_proyek' => 'sometimes|string|max:50',
            'kode_fase' => [
                'sometimes',
                'string',
                'max:30',
                Rule::unique('master_fase_pekerjaans', 'kode_fase')
                    ->ignore($fase->id)
                    ->where(
                        fn ($q) => $q->where(
                            'jenis_proyek',
                            $request->input('jenis_proyek', $fase->jenis_proyek)
                        )
                    ),
            ],
            'nama_fase' => 'sometimes|string|max:100',
            'prioritas' => 'sometimes|integer|min:0',
            'overlap_persen' => 'nullable|integer|min:0|max:100',
            'durasi_faktor' => 'nullable|numeric|min:0.1',
            'keywords' => 'nullable|array',
            'keywords.*' => 'string|max:100',
            'deskripsi' => 'nullable|string',
            'is_active' => 'nullable|boolean',
        ]);

        if (array_key_exists('keywords', $validated) && $validated['keywords'] === null) {
            $validated['keywords'] = [];
        }

        $fase->update($validated);

        return response()->json([
            'success' => true,
            'data' => $fase->fresh(),
        ]);
    }

    public function destroy($id)
    {
        $fase = MasterFasePekerjaan::findOrFail($id);
        $fase->delete();

        return response()->json([
            'success' => true,
            'message' => 'Data deleted successfully',
        ]);
    }
}
