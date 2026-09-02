<?php

namespace App\Http\Controllers;

use App\Http\Resources\PetaPeripaanResource;
use App\Models\PetaPeripaan;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class PetaPeripaanController extends Controller
{
    public function index(Request $request)
    {
        $query = PetaPeripaan::with(['pekerjaan:id,nama_paket', 'uploader:id,name'])
            ->latest();

        $query->when($request->filled('pekerjaan_id'), fn ($q) => $q->where('pekerjaan_id', $request->pekerjaan_id));

        $perPage = $request->input('per_page', 50);
        $items = ($perPage == -1)
            ? $query->get()
            : $query->paginate((int) $perPage);

        return PetaPeripaanResource::collection($items);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'nama' => 'required|string|max:255',
            'geojson' => 'nullable|json',
            'file' => 'required|file|max:51200', // max 50MB (KMZ=zip, KML=xml)
        ]);

        $peripaan = PetaPeripaan::create([
            'pekerjaan_id' => $validated['pekerjaan_id'] ?? null,
            'nama' => $validated['nama'],
            // multipart mengirim geojson sebagai string JSON — decode agar tak double-encoded
            'geojson' => isset($validated['geojson']) ? json_decode($validated['geojson'], true) : null,
            'uploaded_by' => $request->user()?->id,
        ]);

        if ($request->hasFile('file')) {
            $originalName = $request->file('file')->getClientOriginalName();
            $peripaan->addMediaFromRequest('file')
                ->usingName(pathinfo($originalName, PATHINFO_FILENAME) ?: $originalName)
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('peripaan/kml');
        }

        $peripaan->load(['pekerjaan:id,nama_paket', 'uploader:id,name']);
        return new PetaPeripaanResource($peripaan);
    }

    public function destroy(PetaPeripaan $peripaan): JsonResponse
    {
        $peripaan->delete();

        return response()->json(['message' => 'Peta peripaan deleted']);
    }
}
