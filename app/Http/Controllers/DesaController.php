<?php

namespace App\Http\Controllers;

use App\Models\Desa;
use App\Http\Resources\DesaResource;
use Illuminate\Http\Request;

class DesaController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index()
    {
        $desa = Desa::with('kecamatan')->paginate(15);
        return DesaResource::collection($desa);
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'nama_desa' => 'required|string|max:100',
            'luas' => 'required|numeric',
            'jumlah_penduduk' => 'required|integer',
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
     * Display the specified resource.
     */
    public function show(Desa $desa)
    {
        $desa->load('kecamatan');
        return new DesaResource($desa);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, Desa $desa)
    {
        $validated = $request->validate([
            'nama_desa' => 'nullable|string|max:100',
            'luas' => 'nullable|numeric',
            'jumlah_penduduk' => 'nullable|integer',
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
     * Remove the specified resource from storage.
     */
    public function destroy(Desa $desa)
    {
        $desa->delete();
        return response()->json(['message' => 'Desa deleted successfully'], 200);
    }

    /**
     * Get desa by kecamatan
     */
    public function byKecamatan($kecamatanId)
    {
        $desa = Desa::where('kecamatan_id', $kecamatanId)->get();
        return DesaResource::collection($desa);
    }
}
