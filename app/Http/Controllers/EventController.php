<?php

namespace App\Http\Controllers;

use App\Models\Event;
use App\Http\Resources\EventResource;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Auth;

class EventController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/events",
     *     summary="List all events for current user",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index()
    {
        $events = Event::where('user_id', Auth::id())->get();
        return EventResource::collection($events);
    }

    /**
     * @OA\Post(
     *     path="/api/events",
     *     summary="Create new event",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"title", "start", "end"},
     *             @OA\Property(property="title", type="string"),
     *             @OA\Property(property="start", type="string", format="date-time"),
     *             @OA\Property(property="end", type="string", format="date-time"),
     *             @OA\Property(property="is_allday", type="boolean"),
     *             @OA\Property(property="category", type="string", enum={"event","task","milestone","holiday"}),
     *             @OA\Property(property="location", type="string"),
     *             @OA\Property(property="description", type="string")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Event created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'title' => 'required|string|max:255',
            'is_allday' => 'boolean',
            'start' => 'required|date',
            'end' => 'required|date|after_or_equal:start',
            'category' => 'string|in:event,task,milestone,holiday',
            'location' => 'nullable|string|max:255',
            'description' => 'nullable|string',
            'color' => 'nullable|string|max:20',
            'bg_color' => 'nullable|string|max:20',
            'border_color' => 'nullable|string|max:20',
        ]);

        $event = Event::create(array_merge($validated, ['user_id' => Auth::id()]));

        return new EventResource($event);
    }

    /**
     * @OA\Get(
     *     path="/api/events/{id}",
     *     summary="Get event detail",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Event $event)
    {
        if ($event->user_id !== Auth::id()) {
            abort(403);
        }
        return new EventResource($event);
    }

    /**
     * @OA\Put(
     *     path="/api/events/{id}",
     *     summary="Update event",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, Event $event)
    {
        if ($event->user_id !== Auth::id()) {
            abort(403);
        }

        $validated = $request->validate([
            'title' => 'sometimes|required|string|max:255',
            'is_allday' => 'boolean',
            'start' => 'sometimes|required|date',
            'end' => 'sometimes|required|date|after_or_equal:start',
            'category' => 'string|in:event,task,milestone,holiday',
            'location' => 'nullable|string|max:255',
            'description' => 'nullable|string',
            'color' => 'nullable|string|max:20',
            'bg_color' => 'nullable|string|max:20',
            'border_color' => 'nullable|string|max:20',
        ]);

        $event->update($validated);

        return new EventResource($event);
    }

    /**
     * @OA\Delete(
     *     path="/api/events/{id}",
     *     summary="Delete event",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Event $event)
    {
        if ($event->user_id !== Auth::id()) {
            abort(403);
        }

        $event->delete();

        return response()->json(['message' => 'Event deleted successfully']);
    }
}
