<?php

namespace App\Http\Controllers;

use App\Models\Kegiatan;
use App\Http\Resources\KegiatanResource;
use Illuminate\Http\Request;

class KegiatanController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(Request $request)
    {
        $query = Kegiatan::query();

        if ($request->has('tahun') && $request->tahun) {
            $query->where('tahun_anggaran', $request->tahun);
        }

        $kegiatan = $query->paginate(15);
        return KegiatanResource::collection($kegiatan);
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'nama_program' => 'nullable|string|max:255',
            'nama_kegiatan' => 'nullable|string|max:255',
            'nama_sub_kegiatan' => 'nullable|string|max:255',
            'tahun_anggaran' => 'nullable|string|max:50',
            'sumber_dana' => 'nullable|string|max:255',
            'pagu' => 'nullable|numeric|min:0',
            'kode_rekening' => 'nullable|array'
        ]);

        $kegiatan = Kegiatan::create($validated);
        return new KegiatanResource($kegiatan);
    }

    /**
     * Display the specified resource.
     */
    public function show(Kegiatan $kegiatan)
    {
        return new KegiatanResource($kegiatan);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, Kegiatan $kegiatan)
    {
        $validated = $request->validate([
            'nama_program' => 'nullable|string|max:255',
            'nama_kegiatan' => 'nullable|string|max:255',
            'nama_sub_kegiatan' => 'nullable|string|max:255',
            'tahun_anggaran' => 'nullable|string|max:50',
            'sumber_dana' => 'nullable|string|max:255',
            'pagu' => 'nullable|numeric|min:0',
            'kode_rekening' => 'nullable|array'
        ]);

        $kegiatan->update($validated);
        return new KegiatanResource($kegiatan);
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(Kegiatan $kegiatan)
    {
        $kegiatan->delete();
        return response()->json(['message' => 'Kegiatan deleted successfully'], 200);
    }

    /**
     * Filter kegiatan by tahun anggaran
     */
    public function byTahun($tahun)
    {
        $kegiatan = Kegiatan::where('tahun_anggaran', $tahun)->paginate(15);
        return KegiatanResource::collection($kegiatan);
    }
}
