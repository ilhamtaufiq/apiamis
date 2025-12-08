<?php

namespace App\Http\Controllers;

use App\Models\Penerima;
use App\Http\Resources\PenerimaResource;
use Illuminate\Http\Request;

class PenerimaController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(Request $request)
    {
        $query = Penerima::with('pekerjaan');

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        // Filter by pekerjaan_id
        if ($request->filled('pekerjaan_id')) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
        }

        // Filter by komunal
        if ($request->boolean('komunal') !== null) {
            $query->komunal($request->boolean('komunal'));
        }

        // Search by nama
        if ($request->filled('search')) {
            $query->searchNama($request->search);
        }

        $penerima = $query->paginate(20);

        return PenerimaResource::collection($penerima);
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|integer|exists:tbl_pekerjaan,id',
            'nama' => 'required|string|max:255',
            'jumlah_jiwa' => 'nullable|integer|min:1',
            'nik' => 'nullable|string|max:255',
            'alamat' => 'nullable|string|max:255',
            'is_komunal' => 'boolean',
        ]);

        $penerima = Penerima::create($validated);
        $penerima->load('pekerjaan');

        return new PenerimaResource($penerima);
    }

    /**
     * Display the specified resource.
     */
    public function show(Penerima $penerima)
    {
        $penerima->load('pekerjaan');
        return new PenerimaResource($penerima);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, Penerima $penerima)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|integer|exists:tbl_pekerjaan,id',
            'nama' => 'nullable|string|max:255',
            'jumlah_jiwa' => 'nullable|integer|min:1',
            'nik' => 'nullable|string|max:255',
            'alamat' => 'nullable|string|max:255',
            'is_komunal' => 'nullable|boolean',
        ]);

        $penerima->update($validated);
        $penerima->load('pekerjaan');

        return new PenerimaResource($penerima);
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(Penerima $penerima)
    {
        $penerima->delete();
        return response()->json(['message' => 'Penerima berhasil dihapus'], 200);
    }

    /**
     * Get penerima by pekerjaan
     */
    public function byPekerjaan($pekerjaanId)
    {
        $penerima = Penerima::where('pekerjaan_id', $pekerjaanId)
            ->with('pekerjaan')
            ->paginate(50);

        return PenerimaResource::collection($penerima);
    }

    /**
     * Get komunal penerima count by pekerjaan
     */
    public function komunalCount($pekerjaanId)
    {
        $total = Penerima::where('pekerjaan_id', $pekerjaanId)->count();
        $komunal = Penerima::where('pekerjaan_id', $pekerjaanId)->komunal(true)->count();

        return response()->json([
            'pekerjaan_id' => $pekerjaanId,
            'total_penerima' => $total,
            'komunal_count' => $komunal,
            'non_komunal_count' => $total - $komunal,
        ]);
    }
}
    