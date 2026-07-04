<?php

namespace App\Http\Controllers;

use App\Models\Penyedia;
use App\Http\Resources\PenyediaResource;
use Illuminate\Http\Request;

class PenyediaController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/penyedia",
     *     summary="List all penyedia",
     *     tags={"Penyedia"},
     *     @OA\Parameter(name="search", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
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
                  ->orWhere('notaris', 'like', "%{$search}%")
                  ->orWhere('npwp', 'like', "%{$search}%");
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
     * @OA\Post(
     *     path="/api/penyedia",
     *     summary="Create new penyedia",
     *     tags={"Penyedia"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 required={"nama", "direktur", "no_akta", "notaris", "tanggal_akta", "alamat"},
     *                 @OA\Property(property="nama", type="string"),
     *                 @OA\Property(property="direktur", type="string"),
     *                 @OA\Property(property="no_akta", type="string"),
     *                 @OA\Property(property="notaris", type="string"),
     *                 @OA\Property(property="tanggal_akta", type="string", format="date"),
     *                 @OA\Property(property="alamat", type="string"),
     *                 @OA\Property(property="bank", type="string"),
     *                 @OA\Property(property="norek", type="string")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=201, description="Penyedia created")
     * )
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
            'npwp' => 'nullable|string|max:32',
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
     * @OA\Get(
     *     path="/api/penyedia/{id}",
     *     summary="Get penyedia detail",
     *     tags={"Penyedia"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Penyedia $penyedia)
    {
        return new PenyediaResource($penyedia);
    }

    /**
     * @OA\Post(
     *     path="/api/penyedia/{id}",
     *     summary="Update penyedia",
     *     description="Uses POST with _method=PUT for multipart/form-data support",
     *     tags={"Penyedia"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Penyedia updated")
     * )
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
            'npwp' => 'nullable|string|max:32',
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
     * @OA\Delete(
     *     path="/api/penyedia/{id}",
     *     summary="Delete penyedia",
     *     tags={"Penyedia"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Penyedia deleted")
     * )
     */
    public function destroy(Penyedia $penyedia)
    {
        $penyedia->delete();
        return response()->json(['message' => 'Penyedia deleted successfully'], 200);
    }
}
