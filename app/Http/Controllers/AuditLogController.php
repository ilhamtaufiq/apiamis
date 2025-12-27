<?php

namespace App\Http\Controllers;

use App\Models\AuditLog;
use Illuminate\Http\Request;

class AuditLogController extends Controller
{
    /**
     * Display a listing of the audit logs.
     *
     * @param  \Illuminate\Http\Request  $request
     * @return \Illuminate\Http\Response
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
     * Display the specified audit log.
     *
     * @param  \App\Models\AuditLog  $auditLog
     * @return \Illuminate\Http\Response
     */
    public function show(AuditLog $auditLog)
    {
        return response()->json([
            'success' => true,
            'data' => $auditLog->load('user')
        ]);
    }
}
