<?php

namespace App\Http\Controllers;

use App\Models\Penyedia;
use App\Http\Resources\PenyediaResource;
use Illuminate\Http\Request;

class PenyediaController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(Request $request)
    {
        $query = Penyedia::query();
        
        if ($request->filled('search')) {
            $search = $request->search;
            $query->where(function($q) use ($search) {
                $q->where('nama', 'like', "%{$search}%")
                  ->orWhere('direktur', 'like', "%{$search}%")
                  ->orWhere('alamat', 'like', "%{$search}%")
                  ->orWhere('notaris', 'like', "%{$search}%");
            });
        }

        // Support fetching all records for dropdown (per_page=-1)
        if ($request->has('per_page') && $request->per_page == -1) {
            return PenyediaResource::collection($query->get());
        }
        
        $perPage = $request->get('per_page', 15);
        $penyedia = $query->paginate($perPage);
        return PenyediaResource::collection($penyedia);
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'nama' => 'required|string|max:255',
            'direktur' => 'required|string|max:255',
            'no_akta' => 'required|string|max:255',
            'notaris' => 'required|string|max:255',
            'tanggal_akta' => 'required|date',
            'alamat' => 'required|string|max:255',
            'bank' => 'nullable|string|max:255',
            'norek' => 'nullable|string|max:255',
            'dokumen' => 'nullable|array',
            'dokumen.*' => 'nullable|file|max:51200',
        ]);

        $penyedia = Penyedia::create($validated);

        if ($request->hasFile('dokumen')) {
            foreach ($request->file('dokumen') as $file) {
                $penyedia->addMedia($file)
                    ->usingFileName(\Illuminate\Support\Str::uuid() . '.' . $file->getClientOriginalExtension())
                    ->toMediaCollection('penyedia/dokumen');
            }
        }

        $penyedia->load('media');
        return new PenyediaResource($penyedia);
    }

    /**
     * Display the specified resource.
     */
    public function show(Penyedia $penyedia)
    {
        return new PenyediaResource($penyedia);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, Penyedia $penyedia)
    {
        $validated = $request->validate([
            'nama' => 'required|string|max:255',
            'direktur' => 'required|string|max:255',
            'no_akta' => 'required|string|max:255',
            'notaris' => 'required|string|max:255',
            'tanggal_akta' => 'required|date',
            'alamat' => 'required|string|max:255',
            'bank' => 'nullable|string|max:255',
            'norek' => 'nullable|string|max:255',
            'dokumen' => 'nullable|array',
            'dokumen.*' => 'nullable|file|max:51200',
            'delete_dokumen' => 'nullable|array',
            'delete_dokumen.*' => 'nullable|integer',
        ]);

        $penyedia->update($validated);

        if ($request->hasFile('dokumen')) {
            foreach ($request->file('dokumen') as $file) {
                $penyedia->addMedia($file)
                    ->usingFileName(\Illuminate\Support\Str::uuid() . '.' . $file->getClientOriginalExtension())
                    ->toMediaCollection('penyedia/dokumen');
            }
        }

        if ($request->filled('delete_dokumen')) {
            $penyedia->media()->whereIn('id', $request->input('delete_dokumen'))->delete();
        }

        $penyedia->load('media');
        return new PenyediaResource($penyedia);
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(Penyedia $penyedia)
    {
        $penyedia->delete();
        return response()->json(['message' => 'Penyedia deleted successfully'], 200);
    }
}
