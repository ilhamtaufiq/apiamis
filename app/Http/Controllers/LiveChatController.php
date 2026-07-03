<?php

namespace App\Http\Controllers;

use App\Events\LiveChat\InboxUpdated;
use App\Events\LiveChat\MessageSent;
use App\Events\LiveChat\ThreadStatusUpdated;
use App\Http\Resources\LiveChatMessageResource;
use App\Http\Resources\LiveChatThreadResource;
use App\Models\LiveChatMessage;
use App\Models\LiveChatThread;
use App\Models\User;
use App\Notifications\AppNotification;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Notification;
use Illuminate\Support\Facades\Validator;

class LiveChatController extends Controller
{
    public function myThread(Request $request)
    {
        $user = $request->user();

        $thread = LiveChatThread::query()
            ->with(['user', 'latestMessage.user'])
            ->firstOrCreate(
                ['user_id' => $user->id],
                ['status' => 'open'],
            );

        return response()->json([
            'success' => true,
            'data' => new LiveChatThreadResource($thread),
        ]);
    }

    public function inbox(Request $request)
    {
        $user = $request->user();

        if (!$user->hasRole('admin')) {
            return response()->json(['message' => 'Akses ditolak'], 403);
        }

        $threads = LiveChatThread::query()
            ->with(['user', 'latestMessage.user'])
            ->withCount([
                'messages as unread_count' => function ($query) use ($user) {
                    $query
                        ->whereNull('read_at')
                        ->where('user_id', '!=', $user->id);
                },
            ])
            ->orderByDesc('last_message_at')
            ->orderByDesc('updated_at')
            ->get();

        return response()->json([
            'success' => true,
            'data' => LiveChatThreadResource::collection($threads),
        ]);
    }

    public function messages(Request $request, LiveChatThread $thread)
    {
        $user = $request->user();

        if (!$this->canAccessThread($user, $thread)) {
            return response()->json(['message' => 'Akses ditolak'], 403);
        }

        $afterId = (int) $request->query('after_id', 0);

        $query = $thread->messages()
            ->with('user')
            ->orderBy('id');

        if ($afterId > 0) {
            $query->where('id', '>', $afterId);
        }

        $messages = $query->get();

        $this->markMessagesAsRead($thread, $user);

        return response()->json([
            'success' => true,
            'data' => LiveChatMessageResource::collection($messages),
        ]);
    }

    public function sendMessage(Request $request, LiveChatThread $thread)
    {
        $user = $request->user();

        if (!$this->canAccessThread($user, $thread)) {
            return response()->json(['message' => 'Akses ditolak'], 403);
        }

        $validator = Validator::make($request->all(), [
            'message' => 'required|string|max:5000',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $message = DB::transaction(function () use ($request, $thread, $user) {
            $message = $thread->messages()->create([
                'user_id' => $user->id,
                'message' => trim($request->message),
            ]);

            $thread->update([
                'status' => 'open',
                'last_message_at' => now(),
            ]);

            return $message;
        });

        $message->load('user');

        $this->notifyRecipients($thread, $user, $message);

        broadcast(new MessageSent($message));
        broadcast(new InboxUpdated($thread->id));

        return response()->json([
            'success' => true,
            'data' => new LiveChatMessageResource($message),
        ], 201);
    }

    public function closeThread(Request $request, LiveChatThread $thread)
    {
        $user = $request->user();

        if (!$this->canAccessThread($user, $thread)) {
            return response()->json(['message' => 'Akses ditolak'], 403);
        }

        $thread->update(['status' => 'closed']);
        $thread->load(['user', 'latestMessage.user']);

        broadcast(new ThreadStatusUpdated($thread));
        broadcast(new InboxUpdated($thread->id));

        return response()->json([
            'success' => true,
            'data' => new LiveChatThreadResource($thread),
        ]);
    }

    private function canAccessThread(User $user, LiveChatThread $thread): bool
    {
        return $user->hasRole('admin') || $thread->user_id === $user->id;
    }

    private function markMessagesAsRead(LiveChatThread $thread, User $user): void
    {
        LiveChatMessage::query()
            ->where('thread_id', $thread->id)
            ->where('user_id', '!=', $user->id)
            ->whereNull('read_at')
            ->update(['read_at' => now()]);
    }

    private function notifyRecipients(LiveChatThread $thread, User $sender, LiveChatMessage $message): void
    {
        $preview = mb_strlen($message->message) > 80
            ? mb_substr($message->message, 0, 80) . '...'
            : $message->message;

        if ($sender->hasRole('admin')) {
            $thread->user?->notify(new AppNotification(
                'Balasan Live Chat Admin',
                $sender->name . ': ' . $preview,
                '/dashboard',
                'info',
            ));

            return;
        }

        $admins = User::role('admin')->get();
        Notification::send($admins, new AppNotification(
            'Pesan Live Chat Baru',
            $sender->name . ': ' . $preview,
            '/dashboard',
            'info',
        ));
    }
}