<?php

namespace App\Http\Controllers;

use App\Models\ErrorLog;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Validation\Rule;

class ClientErrorReportController extends Controller
{
    public function index(Request $request): JsonResponse
    {
        $query = ErrorLog::with('user')->latest();

        if ($request->filled('source')) {
            $query->where('source', $request->string('source'));
        }

        if ($request->filled('status')) {
            if ($request->string('status')->toString() === 'resolved') {
                $query->whereNotNull('resolved_at');
            }

            if ($request->string('status')->toString() === 'open') {
                $query->whereNull('resolved_at');
            }
        }

        if ($request->filled('search')) {
            $search = $request->string('search')->toString();
            $query->where(function ($q) use ($search) {
                $q->where('message', 'LIKE', "%{$search}%")
                    ->orWhere('url', 'LIKE', "%{$search}%")
                    ->orWhere('source', 'LIKE', "%{$search}%");
            });
        }

        $perPage = min((int) $request->get('per_page', 15), 100);
        $logs = $query->paginate($perPage);

        return response()->json([
            'success' => true,
            'data' => $logs->items(),
            'meta' => [
                'current_page' => $logs->currentPage(),
                'last_page' => $logs->lastPage(),
                'per_page' => $logs->perPage(),
                'total' => $logs->total(),
                'from' => $logs->firstItem(),
                'to' => $logs->lastItem(),
            ],
        ]);
    }

    public function show(ErrorLog $errorLog): JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => $errorLog->load('user'),
        ]);
    }

    public function store(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'source' => [
                'required',
                'string',
                Rule::in([
                    'react',
                    'react-native',
                    'window.error',
                    'unhandledrejection',
                    'console.error',
                    'manual',
                    'fatal',
                ]),
            ],
            'message' => ['required', 'string', 'max:5000'],
            'stack' => ['nullable', 'string'],
            'component_stack' => ['nullable', 'string'],
            'url' => ['nullable', 'string', 'max:5000'],
            'user_agent' => ['nullable', 'string', 'max:2000'],
            'metadata' => ['nullable', 'array'],
            'app' => ['nullable', 'string', 'max:64'],
        ]);

        $metadata = $validated['metadata'] ?? [];
        if (! empty($validated['app'])) {
            $metadata['app'] = $validated['app'];
        }
        unset($validated['app']);

        ErrorLog::create([
            ...$validated,
            'metadata' => $metadata ?: null,
            'user_id' => $request->user()?->id,
            'ip_address' => $request->ip(),
        ]);

        return response()->json([
            'success' => true,
        ], 201);
    }

    public function resolve(ErrorLog $errorLog): JsonResponse
    {
        $errorLog->update([
            'resolved_at' => now(),
        ]);

        return response()->json([
            'success' => true,
            'data' => $errorLog->fresh('user'),
        ]);
    }

    public function reopen(ErrorLog $errorLog): JsonResponse
    {
        $errorLog->update([
            'resolved_at' => null,
        ]);

        return response()->json([
            'success' => true,
            'data' => $errorLog->fresh('user'),
        ]);
    }

    public function bulkResolve(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'ids' => ['required', 'array', 'min:1'],
            'ids.*' => ['integer', 'exists:error_logs,id'],
        ]);

        $affected = ErrorLog::query()
            ->whereIn('id', $validated['ids'])
            ->whereNull('resolved_at')
            ->update(['resolved_at' => now()]);

        return response()->json([
            'success' => true,
            'affected' => $affected,
        ]);
    }

    public function bulkReopen(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'ids' => ['required', 'array', 'min:1'],
            'ids.*' => ['integer', 'exists:error_logs,id'],
        ]);

        $affected = ErrorLog::query()
            ->whereIn('id', $validated['ids'])
            ->whereNotNull('resolved_at')
            ->update(['resolved_at' => null]);

        return response()->json([
            'success' => true,
            'affected' => $affected,
        ]);
    }

    public function bulkDestroy(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'ids' => ['required', 'array', 'min:1'],
            'ids.*' => ['integer', 'exists:error_logs,id'],
        ]);

        $affected = ErrorLog::query()
            ->whereIn('id', $validated['ids'])
            ->delete();

        return response()->json([
            'success' => true,
            'affected' => $affected,
        ]);
    }

    public function destroyAll(): JsonResponse
    {
        $affected = ErrorLog::query()->delete();

        return response()->json([
            'success' => true,
            'affected' => $affected,
        ]);
    }
}
