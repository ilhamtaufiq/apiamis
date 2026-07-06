<?php

namespace App\Http\Controllers;

use App\Services\UserPresenceService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

class UserPresenceController extends Controller
{
    public function __construct(private readonly UserPresenceService $presence) {}

    public function heartbeat(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'app' => ['sometimes', 'string', 'max:32'],
            'koordinat' => ['sometimes', 'nullable', 'string', 'max:64', 'regex:/^-?\d+(?:\.\d+)?\s*,\s*-?\d+(?:\.\d+)?$/'],
        ]);

        $app = (string) ($validated['app'] ?? 'portal');
        $koordinat = isset($validated['koordinat']) ? trim((string) $validated['koordinat']) : null;
        if ($koordinat === '') {
            $koordinat = null;
        }

        $this->presence->heartbeat($request->user(), $app, $koordinat);

        return response()->json([
            'data' => [
                'ok' => true,
                'online_window_minutes' => UserPresenceService::ONLINE_WINDOW_MINUTES,
            ],
        ]);
    }

    public function index(): JsonResponse
    {
        return response()->json([
            'data' => $this->presence->listOnline(),
            'meta' => [
                'online_window_minutes' => UserPresenceService::ONLINE_WINDOW_MINUTES,
            ],
        ]);
    }
}