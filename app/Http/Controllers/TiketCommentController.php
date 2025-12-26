<?php

namespace App\Http\Controllers;

use App\Models\Tiket;
use App\Models\TiketComment;
use App\Models\User;
use App\Http\Resources\TiketCommentResource;
use App\Notifications\AppNotification;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Notification;
use Illuminate\Support\Facades\Validator;

class TiketCommentController extends Controller
{
    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request, Tiket $tiket)
    {
        $validator = Validator::make($request->all(), [
            'message' => 'required|string',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $comment = $tiket->comments()->create([
            'user_id' => auth()->id(),
            'message' => $request->message,
        ]);

        // Notify the relevant party
        $recipient = null;
        if (auth()->id() === $tiket->user_id) {
            // If user comments, notify admin
            $admins = User::role('admin')->get();
            Notification::send($admins, new AppNotification(
                'Komentar Baru pada Tiket (' . $tiket->subjek . ')',
                auth()->user()->name . ' menambahkan komentar baru.',
                '/tiket?ticketId=' . $tiket->id,
                'info'
            ));
        } else {
            // If admin (or someone else) comments, notify the ticket owner
            $tiket->user->notify(new AppNotification(
                'Komentar Baru pada Tiket',
                'Ada komentar baru pada tiket Anda: "' . $tiket->subjek . '"',
                '/tiket?ticketId=' . $tiket->id,
                'info'
            ));
        }

        return new TiketCommentResource($comment->load('user'));
    }
}
