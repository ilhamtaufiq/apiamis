<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use App\Http\Resources\DraftPekerjaanResource;
use App\Http\Resources\PekerjaanResource;
use App\Models\DraftPekerjaan;

class DraftPekerjaanController extends Controller
{
    public function index(Request $request)
    {
        $query = \App\Models\Pekerjaan::with(['kecamatan', 'desa', 'draft.penyedia', 'kegiatan'])
            ->byUserRole();

        if ($request->has('tahun') && !empty($request->tahun)) {
            $query->whereHas('kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->has('search') && !empty($request->search)) {
            $searchTerm = $request->search;
            $query->where(function($q) use ($searchTerm) {
                $q->where('nama_paket', 'LIKE', '%' . $searchTerm . '%')
                  ->orWhere('kode_rekening', 'LIKE', '%' . $searchTerm . '%');
            });
        }

        return PekerjaanResource::collection($query->paginate($request->per_page ?? 10));
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'penyedia_id' => 'nullable|exists:tbl_penyedia,id',
            'nama_pelaksana' => 'nullable|string',
            'kode_rup' => 'nullable|string',
            'kode_paket' => 'nullable|string',
        ]);

        // Use updateOrCreate since we are managing ALL pekerjaan and just adding/updating the draft
        $draft = DraftPekerjaan::updateOrCreate(
            ['pekerjaan_id' => $validated['pekerjaan_id']],
            [
                'penyedia_id' => $validated['penyedia_id'] ?? null,
                'nama_pelaksana' => $validated['nama_pelaksana'] ?? null,
                'kode_rup' => $validated['kode_rup'] ?? null,
                'kode_paket' => $validated['kode_paket'] ?? null,
            ]
        );

        return new DraftPekerjaanResource($draft->load('pekerjaan', 'penyedia'));
    }

    public function show($id)
    {
        $draft = DraftPekerjaan::with(['pekerjaan', 'penyedia'])->findOrFail($id);
        return new DraftPekerjaanResource($draft);
    }

    public function update(Request $request, $id)
    {
        $draft = DraftPekerjaan::findOrFail($id);

        $validated = $request->validate([
            'pekerjaan_id' => 'sometimes|required|exists:tbl_pekerjaan,id',
            'penyedia_id' => 'nullable|exists:tbl_penyedia,id',
            'nama_pelaksana' => 'nullable|string',
            'kode_rup' => 'nullable|string',
            'kode_paket' => 'nullable|string',
        ]);

        $draft->update($validated);

        return new DraftPekerjaanResource($draft->load('pekerjaan', 'penyedia'));
    }

    public function destroy($id)
    {
        $draft = \App\Models\DraftPekerjaan::findOrFail($id);
        $draft->delete();

        return response()->json(null, 204);
    }
}
