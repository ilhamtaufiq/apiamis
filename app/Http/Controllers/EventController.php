<?php

namespace App\Http\Controllers;

use App\Models\Event;
use App\Http\Resources\EventResource;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Auth;

class EventController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index()
    {
        $events = Event::where('user_id', Auth::id())->get();
        return EventResource::collection($events);
    }

    /**
     * Store a newly created resource in storage.
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
     * Display the specified resource.
     */
    public function show(Event $event)
    {
        if ($event->user_id !== Auth::id()) {
            abort(403);
        }
        return new EventResource($event);
    }

    /**
     * Update the specified resource in storage.
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
     * Remove the specified resource from storage.
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
