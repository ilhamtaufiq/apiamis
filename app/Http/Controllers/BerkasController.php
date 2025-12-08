<?php

namespace App\Http\Controllers;

use App\Models\Berkas;
use App\Http\Resources\BerkasResource;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class BerkasController extends Controller
{
    public function index(Request $request)
    {
        $query = Berkas::with('pekerjaan');

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->has('pekerjaan_id')) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
        }

        $berkas = $query->paginate(20);
        return BerkasResource::collection($berkas);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'jenis_dokumen' => 'required|string|max:255',
            'file' => 'required|file|max:10240', // max 10MB
        ]);

        $berkas = Berkas::create([
            'pekerjaan_id' => $validated['pekerjaan_id'],
            'jenis_dokumen' => $validated['jenis_dokumen'],
        ]);

        // Upload file dengan Spatie MediaLibrary
        if ($request->hasFile('file')) {
            $berkas->addMediaFromRequest('file')
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('berkas/dokumen');
        }

        $berkas->load('pekerjaan');
        return new BerkasResource($berkas);
    }

    public function show(Berkas $berkas)
    {
        $berkas->load('pekerjaan');
        return new BerkasResource($berkas);
    }

    public function update(Request $request, Berkas $berkas)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'jenis_dokumen' => 'nullable|string|max:255',
            'file' => 'nullable|file|max:10240',
        ]);

        $berkas->update($validated);

        if ($request->hasFile('file')) {
            $berkas->clearMediaCollection('berkas/dokumen');
            $berkas->addMediaFromRequest('file')
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('berkas/dokumen');
        }

        $berkas->load('pekerjaan');
        return new BerkasResource($berkas);
    }

    public function destroy(Berkas $berkas)
    {
        // Delete all media files from storage
        $berkas->clearMediaCollection('berkas/dokumen');
        
        // Delete the database record
        $berkas->delete();
        
        return response()->json(['message' => 'Berkas deleted successfully']);
    }
}
