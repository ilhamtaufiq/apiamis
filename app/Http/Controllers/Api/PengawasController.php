<?php

namespace App\Http\Controllers\Api;

use App\Http\Controllers\Controller;
use Illuminate\Http\Request;

use App\Models\Pengawas;
use App\Http\Resources\PengawasResource;

class PengawasController extends Controller
{
    public function index()
    {
        return PengawasResource::collection(Pengawas::all());
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'nama' => 'required|string|max:255',
            'nip' => 'nullable|string|max:255',
            'jabatan' => 'nullable|string|max:255',
            'telepon' => 'nullable|string|max:255',
        ]);

        $pengawas = Pengawas::create($validated);
        return new PengawasResource($pengawas);
    }

    /**
     * Display the specified resource.
     */
    public function show(Pengawas $pengawa)
    {
        return new PengawasResource($pengawa);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, Pengawas $pengawa)
    {
        $validated = $request->validate([
            'nama' => 'nullable|string|max:255',
            'nip' => 'nullable|string|max:255',
            'jabatan' => 'nullable|string|max:255',
            'telepon' => 'nullable|string|max:255',
        ]);

        $pengawa->update($validated);
        return new PengawasResource($pengawa);
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(Pengawas $pengawa)
    {
        $pengawa->delete();
        return response()->json(['message' => 'Pengawas deleted successfully'], 200);
    }
}
