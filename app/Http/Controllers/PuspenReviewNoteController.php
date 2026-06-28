<?php

namespace App\Http\Controllers;

use App\Http\Resources\PuspenReviewNoteResource;
use App\Models\Pekerjaan;
use App\Models\PuspenReviewNote;
use Illuminate\Http\Request;

class PuspenReviewNoteController extends Controller
{
    public function index(Pekerjaan $pekerjaan)
    {
        $notes = PuspenReviewNote::query()
            ->where('pekerjaan_id', $pekerjaan->id)
            ->with('user:id,name,email')
            ->orderByDesc('created_at')
            ->get();

        return PuspenReviewNoteResource::collection($notes);
    }

    public function store(Request $request, Pekerjaan $pekerjaan)
    {
        $validated = $request->validate([
            'content' => 'required|string|min:1|max:5000',
        ]);

        $note = PuspenReviewNote::create([
            'pekerjaan_id' => $pekerjaan->id,
            'user_id' => auth()->id(),
            'content' => trim($validated['content']),
        ]);

        return (new PuspenReviewNoteResource($note->load('user:id,name,email')))
            ->response()
            ->setStatusCode(201);
    }

    public function destroy(PuspenReviewNote $puspenReviewNote)
    {
        $user = auth()->user();

        if (!$user->hasRole('admin') && $user->id !== $puspenReviewNote->user_id) {
            return response()->json(['message' => 'Anda tidak berhak menghapus catatan ini.'], 403);
        }

        $puspenReviewNote->delete();

        return response()->noContent();
    }
}