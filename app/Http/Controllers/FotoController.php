<?php

namespace App\Http\Controllers;

use App\Models\Foto;
use App\Http\Resources\FotoResource;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class FotoController extends Controller
{
    public function index(Request $request)
    {
        $query = Foto::with(['pekerjaan', 'penerima', 'komponen']);

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->has('pekerjaan_id')) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
            // Return all photos for a specific pekerjaan (no pagination)
            $foto = $query->get();
            return FotoResource::collection($foto);
        }

        // Paginate only when listing all photos
        $foto = $query->paginate(20);
        return FotoResource::collection($foto);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'komponen_id' => 'required|integer',
            'penerima_id' => 'nullable|integer',
            'keterangan' => 'required|in:0%,25%,50%,75%,100%',
            'koordinat' => 'required|string|max:255',
            'validasi_koordinat' => 'boolean',
            'validasi_koordinat_message' => 'nullable|string|max:255',
            'file' => 'required|file|mimes:jpg,jpeg,png|max:5120', // Max 5MB and images only
        ]);

        $foto = Foto::create($validated);

        if ($request->hasFile('file')) {
            $foto->addMediaFromRequest('file')
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('foto/pekerjaan');
        }

        $foto->load(['pekerjaan', 'penerima', 'komponen']);
        return new FotoResource($foto);
    }

    public function show(Foto $foto)
    {
        $foto->load(['pekerjaan', 'penerima', 'komponen']);
        return new FotoResource($foto);
    }

    public function update(Request $request, Foto $foto)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'komponen_id' => 'nullable|integer',
            'penerima_id' => 'nullable|integer',
            'keterangan' => 'nullable|in:0%,25%,50%,75%,100%',
            'koordinat' => 'nullable|string|max:255',
            'validasi_koordinat' => 'nullable|boolean',
            'validasi_koordinat_message' => 'nullable|string|max:255',
            'file' => 'nullable|file|mimes:jpg,jpeg,png|max:5120',
        ]);

        $foto->update($validated);

        if ($request->hasFile('file')) {
            $foto->clearMediaCollection('foto/pekerjaan');
            $foto->addMediaFromRequest('file')
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('foto/pekerjaan');
        }

        $foto->load(['pekerjaan', 'penerima', 'komponen']);
        return new FotoResource($foto);
    }

    public function destroy(Foto $foto)
    {
        // Delete all media files from storage
        $foto->clearMediaCollection('foto/pekerjaan');
        
        // Delete the database record
        $foto->delete();
        
        return response()->json(['message' => 'Foto deleted successfully']);
    }
}
