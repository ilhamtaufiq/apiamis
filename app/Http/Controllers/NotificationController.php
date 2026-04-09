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
     * @OA\Get(
     *     path="/api/notifications",
     *     summary="List user notifications",
     *     tags={"Notifications"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="unread_only", in="query", required=false, @OA\Schema(type="boolean")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
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
     * @OA\Post(
     *     path="/api/notifications/{id}/read",
     *     summary="Mark notification as read",
     *     tags={"Notifications"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="string")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function markAsRead($id)
    {
        $notification = auth()->user()->notifications()->findOrFail($id);
        $notification->markAsRead();

        return response()->json(['message' => 'Notification marked as read']);
    }

    /**
     * @OA\Post(
     *     path="/api/notifications/read-all",
     *     summary="Mark all notifications as read",
     *     tags={"Notifications"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function markAllAsRead()
    {
        auth()->user()->unreadNotifications->markAsRead();

        return response()->json(['message' => 'All notifications marked as read']);
    }

    /**
     * @OA\Post(
     *     path="/api/notifications/broadcast",
     *     summary="Send broadcast notification",
     *     tags={"Notifications (Admin Only)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"title", "message", "type"},
     *             @OA\Property(property="title", type="string"),
     *             @OA\Property(property="message", type="string"),
     *             @OA\Property(property="type", type="string", enum={"all", "single", "multiple"}),
     *             @OA\Property(property="user_ids", type="array", @OA\Items(type="integer")),
     *             @OA\Property(property="notification_type", type="string", enum={"info", "success", "warning", "error"}),
     *             @OA\Property(property="url", type="string")
     *         )
     *     ),
     *     @OA\Response(response=200, description="Broadcast success")
     * )
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
