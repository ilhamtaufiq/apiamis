<?php

namespace App\Http\Controllers;

use App\Models\Kecamatan;
use App\Http\Resources\KecamatanResource;
use App\Http\Resources\KecamatanDetailResource;
use Illuminate\Http\Request;

class KecamatanController extends Controller
{
    /**
     * Display a listing of the resource (index).
     */
    public function index()
    {
        $kecamatan = Kecamatan::all();
        return KecamatanResource::collection($kecamatan);
    }

    /**
     * Store a newly created resource in storage.
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
     * Display the specified resource (show).
     */
    public function show(Kecamatan $kecamatan)
    {
        $kecamatan->load('desa');
        return new KecamatanDetailResource($kecamatan);
    }

    /**
     * Update the specified resource in storage.
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
     * Remove the specified resource from storage.
     */
    public function destroy(Kecamatan $kecamatan)
    {
        $kecamatan->delete();
        return response()->json(['message' => 'Kecamatan deleted successfully'], 200);
    }
}
