<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use App\Http\Resources\DraftPekerjaanResource;
use App\Http\Resources\PekerjaanResource;
use App\Models\DraftPekerjaan;

class DraftPekerjaanController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/draft-pekerjaan",
     *     summary="List all draft pekerjaan",
     *     tags={"Draft Pekerjaan"},
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="search", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
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

    /**
     * @OA\Post(
     *     path="/api/draft-pekerjaan",
     *     summary="Create or update draft pekerjaan",
     *     tags={"Draft Pekerjaan"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"pekerjaan_id"},
     *             @OA\Property(property="pekerjaan_id", type="integer"),
     *             @OA\Property(property="penyedia_id", type="integer"),
     *             @OA\Property(property="nama_pelaksana", type="string"),
     *             @OA\Property(property="kode_rup", type="string"),
     *             @OA\Property(property="kode_paket", type="string")
     *         )
     *     ),
     *     @OA\Response(response=200, description="Draft saved")
     * )
     */
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

    /**
     * @OA\Get(
     *     path="/api/draft-pekerjaan/{id}",
     *     summary="Get draft detail",
     *     tags={"Draft Pekerjaan"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show($id)
    {
        $draft = DraftPekerjaan::with(['pekerjaan', 'penyedia'])->findOrFail($id);
        return new DraftPekerjaanResource($draft);
    }

    /**
     * @OA\Put(
     *     path="/api/draft-pekerjaan/{id}",
     *     summary="Update draft",
     *     tags={"Draft Pekerjaan"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
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

    /**
     * @OA\Delete(
     *     path="/api/draft-pekerjaan/{id}",
     *     summary="Delete draft",
     *     tags={"Draft Pekerjaan"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy($id)
    {
        $draft = DraftPekerjaan::findOrFail($id);
        $draft->delete();

        return response()->json(null, 204);
    }
}
