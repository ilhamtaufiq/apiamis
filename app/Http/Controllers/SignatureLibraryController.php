<?php

namespace App\Http\Controllers;

use App\Models\SignatureLibrary;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

class SignatureLibraryController extends Controller
{
    public function index(Request $request): JsonResponse
    {
        $signatures = SignatureLibrary::ownedBy($request->user()->id)
            ->orderByDesc('updated_at')
            ->get();

        return response()->json([
            'success' => true,
            'data' => $signatures,
        ]);
    }

    public function store(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'name' => 'required|string|max:255',
            'mime_type' => 'required|string|max:100',
            'data_url' => [
                'required',
                'string',
                'regex:/^data:image\/(png|jpe?g|webp);base64,/i',
            ],
            'width' => 'required|integer|min:1|max:20000',
            'height' => 'required|integer|min:1|max:20000',
        ]);

        $signature = SignatureLibrary::updateOrCreate(
            [
                'user_id' => $request->user()->id,
                'name' => $validated['name'],
            ],
            [
                'mime_type' => $validated['mime_type'],
                'data_url' => $validated['data_url'],
                'width' => $validated['width'],
                'height' => $validated['height'],
            ]
        );

        return response()->json([
            'success' => true,
            'message' => $signature->wasRecentlyCreated
                ? 'Signature berhasil disimpan'
                : 'Signature berhasil diperbarui',
            'data' => $signature,
        ], $signature->wasRecentlyCreated ? 201 : 200);
    }

    public function destroy(Request $request, int $id): JsonResponse
    {
        $signature = SignatureLibrary::findOrFail($id);

        if (!$signature->canManage($request->user())) {
            return response()->json([
                'success' => false,
                'message' => 'Anda tidak memiliki izin untuk menghapus signature ini',
            ], 403);
        }

        $signature->delete();

        return response()->json([
            'success' => true,
            'message' => 'Signature berhasil dihapus',
        ]);
    }
}
