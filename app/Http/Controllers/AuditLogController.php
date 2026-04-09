<?php

namespace App\Http\Controllers;

use App\Models\AuditLog;
use Illuminate\Http\Request;

class AuditLogController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/audit-logs",
     *     summary="List all audit logs",
     *     tags={"Audit Logs"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="type", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Parameter(name="event", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Parameter(name="user_id", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = AuditLog::with('user')->orderBy('id', 'desc');

        // Optional filtering by auditable_type
        if ($request->has('type')) {
            $query->where('auditable_type', 'like', '%' . $request->type . '%');
        }

        // Optional filtering by event
        if ($request->has('event')) {
            $query->where('event', $request->event);
        }

        // Optional filtering by user
        if ($request->has('user_id')) {
            $query->where('user_id', $request->user_id);
        }

        $perPage = $request->input('per_page', 15);
        $logs = $query->paginate($perPage);

        return response()->json([
            'success' => true,
            'data' => $logs->items(),
            'meta' => [
                'current_page' => $logs->currentPage(),
                'last_page' => $logs->lastPage(),
                'per_page' => $logs->perPage(),
                'total' => $logs->total(),
            ]
        ]);
    }

    /**
     * @OA\Get(
     *     path="/api/audit-logs/{id}",
     *     summary="Get audit log detail",
     *     tags={"Audit Logs"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(AuditLog $auditLog): \Illuminate\Http\JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => $auditLog->load('user')
        ]);
    }
}
