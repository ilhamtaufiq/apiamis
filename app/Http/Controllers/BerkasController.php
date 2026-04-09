<?php

namespace App\Http\Controllers;

use App\Models\Berkas;
use App\Http\Resources\BerkasResource;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class BerkasController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/berkas",
     *     summary="List all berkas",
     *     tags={"Media Management (Berkas)"},
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="pekerjaan_id", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
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

    /**
     * @OA\Post(
     *     path="/api/berkas",
     *     summary="Upload new berkas",
     *     tags={"Media Management (Berkas)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 required={"pekerjaan_id", "jenis_dokumen", "file"},
     *                 @OA\Property(property="pekerjaan_id", type="integer"),
     *                 @OA\Property(property="jenis_dokumen", type="string"),
     *                 @OA\Property(property="file", type="string", format="binary")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=201, description="Berkas uploaded")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'jenis_dokumen' => 'required|string|max:255',
            'file' => 'required|file|max:51200', // max 50MB
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

    /**
     * @OA\Get(
     *     path="/api/berkas/{id}",
     *     summary="Get berkas detail",
     *     tags={"Media Management (Berkas)"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Berkas $berkas)
    {
        $berkas->load('pekerjaan');
        return new BerkasResource($berkas);
    }

    /**
     * @OA\Post(
     *     path="/api/berkas/{id}",
     *     summary="Update berkas",
     *     description="Uses POST with _method=PUT for multipart/form-data support",
     *     tags={"Media Management (Berkas)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, Berkas $berkas)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'jenis_dokumen' => 'nullable|string|max:255',
            'file' => 'nullable|file|max:51200', // max 50MB
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

    /**
     * @OA\Delete(
     *     path="/api/berkas/{id}",
     *     summary="Delete berkas",
     *     tags={"Media Management (Berkas)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Berkas $berkas)
    {
        // Delete all media files from storage
        $berkas->clearMediaCollection('berkas/dokumen');
        
        // Delete the database record
        $berkas->delete();
        
        return response()->json(['message' => 'Berkas deleted successfully']);
    }
}
