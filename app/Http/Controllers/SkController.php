<?php

namespace App\Http\Controllers;

use App\Http\Resources\SkResource;
use App\Models\Sk;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class SkController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/sk",
     *     summary="List SK",
     *     tags={"Pengaturan SK"},
     *     @OA\Parameter(name="search", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = Sk::with('uploader');

        if ($request->filled('search')) {
            $search = $request->input('search');
            $query->where(function ($q) use ($search) {
                $q->where('nomor_sk', 'like', "%{$search}%")
                    ->orWhere('nama', 'like', "%{$search}%");
            });
        }

        $perPage = min(max((int) $request->query('per_page', 20), 1), 100);
        $sk = $query->latest('id')->paginate($perPage);

        return SkResource::collection($sk);
    }

    /**
     * @OA\Post(
     *     path="/api/sk",
     *     summary="Upload SK baru",
     *     tags={"Pengaturan SK"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 required={"nomor_sk", "nama", "file"},
     *                 @OA\Property(property="nomor_sk", type="string"),
     *                 @OA\Property(property="nama", type="string"),
     *                 @OA\Property(property="tanggal_sk", type="string", format="date"),
     *                 @OA\Property(property="file", type="string", format="binary")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=201, description="SK uploaded")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'nomor_sk' => 'required|string|max:255',
            'nama' => 'required|string|max:255',
            'tanggal_sk' => 'nullable|date',
            'file' => 'required|file|max:51200', // max 50MB
        ]);

        $sk = Sk::create([
            'nomor_sk' => $validated['nomor_sk'],
            'nama' => $validated['nama'],
            'tanggal_sk' => $validated['tanggal_sk'] ?? null,
            'uploaded_by' => $request->user()?->id,
        ]);

        if ($request->hasFile('file')) {
            $sk->addMediaFromRequest('file')
                ->usingName($validated['nama'] ?: 'sk')
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('sk/dokumen');
        }

        $sk->load('uploader');
        return new SkResource($sk);
    }

    /**
     * @OA\Get(
     *     path="/api/sk/{id}",
     *     summary="Get SK detail",
     *     tags={"Pengaturan SK"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Sk $sk)
    {
        $sk->load('uploader');
        return new SkResource($sk);
    }

    /**
     * @OA\Post(
     *     path="/api/sk/{id}",
     *     summary="Update SK",
     *     description="Uses POST with _method=PUT for multipart/form-data support",
     *     tags={"Pengaturan SK"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, Sk $sk)
    {
        $validated = $request->validate([
            'nomor_sk' => 'required|string|max:255',
            'nama' => 'required|string|max:255',
            'tanggal_sk' => 'nullable|date',
            'file' => 'nullable|file|max:51200', // max 50MB
        ]);

        $sk->update([
            'nomor_sk' => $validated['nomor_sk'],
            'nama' => $validated['nama'],
            'tanggal_sk' => $validated['tanggal_sk'] ?? null,
        ]);

        if ($request->hasFile('file')) {
            $sk->clearMediaCollection('sk/dokumen');
            $sk->addMediaFromRequest('file')
                ->usingName($validated['nama'] ?: 'sk')
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('sk/dokumen');
        }

        $sk->load('uploader');
        return new SkResource($sk);
    }

    /**
     * @OA\Delete(
     *     path="/api/sk/{id}",
     *     summary="Delete SK",
     *     tags={"Pengaturan SK"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Sk $sk)
    {
        $sk->clearMediaCollection('sk/dokumen');
        $sk->delete();

        return response()->json(['message' => 'SK deleted successfully']);
    }
}
