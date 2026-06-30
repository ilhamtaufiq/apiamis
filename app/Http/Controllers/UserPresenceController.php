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
        $app = (string) $request->input('app', 'portal');
        $this->presence->heartbeat($request->user(), $app);

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