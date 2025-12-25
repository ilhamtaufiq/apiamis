<?php

namespace App\Http\Controllers;

use App\Models\Tiket;
use App\Http\Resources\TiketResource;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Validator;

class TiketController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(Request $request)
    {
        $user = auth()->user();
        $query = Tiket::with(['user', 'pekerjaan']);

        // Jika bukan admin, hanya lihat tiket sendiri
        if (!$user->hasRole('admin')) {
            $query->where('user_id', $user->id);
        }

        // Filter by status
        if ($request->has('status') && $request->status) {
            $query->where('status', $request->status);
        }

        // Filter by kategori
        if ($request->has('kategori') && $request->kategori) {
            $query->where('kategori', $request->kategori);
        }

        // Filter by pekerjaan_id
        if ($request->has('pekerjaan_id') && $request->pekerjaan_id) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
        }

        $perPage = $request->get('per_page', 20);
        $tikets = $query->latest()->paginate($perPage);

        return TiketResource::collection($tikets);
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request)
    {
        $validator = Validator::make($request->all(), [
            'subjek' => 'required|string|max:255',
            'deskripsi' => 'required|string',
            'kategori' => 'required|in:bug,request,other',
            'prioritas' => 'required|in:low,medium,high',
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'attachment' => 'nullable|image|max:2048',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $tiket = Tiket::create([
            'user_id' => auth()->id(),
            'pekerjaan_id' => $request->pekerjaan_id,
            'subjek' => $request->subjek,
            'deskripsi' => $request->deskripsi,
            'kategori' => $request->kategori,
            'prioritas' => $request->prioritas,
            'status' => 'open',
        ]);

        if ($request->hasFile('attachment')) {
            $tiket->addMediaFromRequest('attachment')->toMediaCollection('attachment');
        }

        return new TiketResource($tiket->load(['user', 'pekerjaan']));
    }

    /**
     * Display the specified resource.
     */
    public function show(Tiket $tiket)
    {
        $user = auth()->user();
        
        // Authorization check
        if (!$user->hasRole('admin') && $tiket->user_id !== $user->id) {
            return response()->json(['message' => 'Unauthorized'], 403);
        }

        return new TiketResource($tiket->load(['user', 'pekerjaan']));
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, Tiket $tiket)
    {
        $user = auth()->user();
        $isAdmin = $user->hasRole('admin');

        // Authorization check
        if (!$isAdmin && $tiket->user_id !== $user->id) {
            return response()->json(['message' => 'Unauthorized'], 403);
        }

        $validationRules = [];

        if ($isAdmin) {
            $validationRules = [
                'status' => 'sometimes|in:open,pending,closed',
                'admin_notes' => 'nullable|string',
            ];
        } else {
            // User can only edit if still open
            if ($tiket->status !== 'open') {
                return response()->json(['message' => 'Tiket yang sudah diproses tidak dapat diubah'], 403);
            }
            $validationRules = [
                'subjek' => 'sometimes|string|max:255',
                'deskripsi' => 'sometimes|string',
                'kategori' => 'sometimes|in:bug,request,other',
                'prioritas' => 'sometimes|in:low,medium,high',
                'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
                'attachment' => 'nullable|image|max:2048',
            ];
        }

        $validator = Validator::make($request->all(), $validationRules);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        if ($isAdmin) {
            $tiket->update($request->only(['status', 'admin_notes']));
        } else {
            $tiket->update($request->except(['attachment', '_method']));
            
            if ($request->hasFile('attachment')) {
                $tiket->clearMediaCollection('attachment');
                $tiket->addMediaFromRequest('attachment')->toMediaCollection('attachment');
            }
        }

        return new TiketResource($tiket->load(['user', 'pekerjaan']));
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(Tiket $tiket)
    {
        $user = auth()->user();

        // Admin or owner can delete, but owner only if still open
        if ($user->hasRole('admin')) {
            $tiket->delete();
            return response()->json(['message' => 'Tiket berhasil dihapus']);
        }

        if ($tiket->user_id === $user->id) {
            if ($tiket->status !== 'open') {
                 return response()->json(['message' => 'Tiket yang sudah diproses tidak dapat dihapus'], 403);
            }
            $tiket->delete();
            return response()->json(['message' => 'Tiket berhasil dihapus']);
        }

        return response()->json(['message' => 'Unauthorized'], 403);
    }
}
