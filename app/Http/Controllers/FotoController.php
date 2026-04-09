<?php

namespace App\Http\Controllers;

use App\Models\Foto;
use App\Http\Resources\FotoResource;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class FotoController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/foto",
     *     summary="List all foto",
     *     tags={"Media Management (Foto)"},
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="pekerjaan_id", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="latest_only", in="query", required=false, @OA\Schema(type="boolean")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
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

        if ($request->has('latest_only') && $request->latest_only) {
            $query->whereIn('id', function ($q) {
                $q->selectRaw('MAX(id)')
                    ->from('tbl_foto')
                    ->groupBy('pekerjaan_id');
            });
        }

        // Support for custom pagination or all results
        $per_page = $request->get('per_page', 20);
        
        if ($per_page == -1) {
            $foto = $query->get();
        } else {
            $foto = $query->paginate($per_page);
        }
        
        return FotoResource::collection($foto);
    }

    /**
     * @OA\Post(
     *     path="/api/foto",
     *     summary="Upload new foto",
     *     tags={"Media Management (Foto)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 required={"pekerjaan_id", "komponen_id", "keterangan", "koordinat", "file"},
     *                 @OA\Property(property="pekerjaan_id", type="integer"),
     *                 @OA\Property(property="komponen_id", type="integer"),
     *                 @OA\Property(property="penerima_id", type="integer"),
     *                 @OA\Property(property="keterangan", type="string", enum={"0%","25%","50%","75%","100%"}),
     *                 @OA\Property(property="koordinat", type="string"),
     *                 @OA\Property(property="file", type="string", format="binary")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=201, description="Foto uploaded")
     * )
     */
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

    /**
     * @OA\Get(
     *     path="/api/foto/{id}",
     *     summary="Get foto detail",
     *     tags={"Media Management (Foto)"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Foto $foto)
    {
        $foto->load(['pekerjaan', 'penerima', 'komponen']);
        return new FotoResource($foto);
    }

    /**
     * @OA\Post(
     *     path="/api/foto/{id}",
     *     summary="Update foto",
     *     description="Uses POST with _method=PUT for multipart/form-data support",
     *     tags={"Media Management (Foto)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
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

    /**
     * @OA\Delete(
     *     path="/api/foto/{id}",
     *     summary="Delete foto",
     *     tags={"Media Management (Foto)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Foto $foto)
    {
        // Delete all media files from storage
        $foto->clearMediaCollection('foto/pekerjaan');
        
        // Delete the database record
        $foto->delete();
        
        return response()->json(['message' => 'Foto deleted successfully']);
    }
}
