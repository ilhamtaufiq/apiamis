<?php

namespace App\Http\Controllers;

use App\Models\User;
use App\Notifications\AppNotification;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Notification;
use Illuminate\Support\Facades\Validator;

class NotificationController extends Controller
{
    /**
     * Display a listing of notifications.
     */
    public function index(Request $request)
    {
        $user = auth()->user();
        $unreadOnly = $request->query('unread_only') === 'true';

        if ($unreadOnly) {
            $notifications = $user->unreadNotifications()->latest()->take(50)->get();
        } else {
            // For notification center, use pagination
            $notifications = $user->notifications()->latest()->paginate(20);
        }

        return response()->json([
            'notifications' => $notifications,
            'unread_count' => $user->unreadNotifications()->count()
        ]);
    }

    /**
     * Mark a notification as read.
     */
    public function markAsRead($id)
    {
        $notification = auth()->user()->notifications()->findOrFail($id);
        $notification->markAsRead();

        return response()->json(['message' => 'Notification marked as read']);
    }

    /**
     * Mark all notifications as read.
     */
    public function markAllAsRead()
    {
        auth()->user()->unreadNotifications->markAsRead();

        return response()->json(['message' => 'All notifications marked as read']);
    }

    /**
     * Send a broadcast notification to users.
     */
    public function sendBroadcast(Request $request)
    {
        $validator = Validator::make($request->all(), [
            'title' => 'required|string|max:255',
            'message' => 'required|string',
            'type' => 'required|in:all,single,multiple',
            'user_ids' => 'required_if:type,single,multiple|array',
            'user_ids.*' => 'exists:users,id',
            'notification_type' => 'nullable|in:info,success,warning,error',
            'url' => 'nullable|string',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $title = $request->title;
        $message = $request->message;
        $url = $request->url;
        $notificationType = $request->notification_type ?? 'info';

        $recipients = null;

        if ($request->type === 'all') {
            $recipients = User::all();
        } else {
            $recipients = User::whereIn('id', $request->user_ids)->get();
        }

        if ($recipients->isEmpty()) {
            return response()->json(['message' => 'No recipients found'], 404);
        }

        Notification::send($recipients, new AppNotification(
            $title,
            $message,
            $url,
            $notificationType
        ));

        return response()->json([
            'message' => 'Notification broadcasted successfully',
            'recipient_count' => $recipients->count()
        ]);
    }
}
