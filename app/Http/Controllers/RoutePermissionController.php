<?php

namespace App\Http\Controllers;

use App\Models\RoutePermission;
use Illuminate\Http\Request;

class RoutePermissionController extends Controller
{
    /**
     * Display a listing of route permissions.
     */
    public function index(Request $request)
    {
        $query = RoutePermission::query();

        // Add search functionality
        if ($request->has('search') && $request->search) {
            $query->where(function($q) use ($request) {
                $q->where('route_path', 'like', '%' . $request->search . '%')
                  ->orWhere('description', 'like', '%' . $request->search . '%');
            });
        }

        // Filter by method if provided
        if ($request->has('method') && $request->method) {
            $query->where('route_method', strtoupper($request->method));
        }

        // Filter by active status
        if ($request->has('is_active')) {
            $query->where('is_active', $request->boolean('is_active'));
        }

        return $query->orderBy('route_path')->paginate(15);
    }

    /**
     * Store a newly created route permission.
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'route_path' => 'required|string|max:255',
            'route_method' => 'required|string|in:GET,POST,PUT,PATCH,DELETE',
            'description' => 'nullable|string',
            'allowed_roles' => 'required|array',
            'allowed_roles.*' => 'string|exists:roles,name',
            'is_active' => 'boolean',
        ]);

        // Check if route permission already exists
        $exists = RoutePermission::where('route_path', $validated['route_path'])
            ->where('route_method', $validated['route_method'])
            ->exists();

        if ($exists) {
            return response()->json([
                'message' => 'Route permission already exists for this path and method'
            ], 422);
        }

        $routePermission = RoutePermission::create($validated);

        return response()->json($routePermission, 201);
    }

    /**
     * Display the specified route permission.
     */
    public function show(RoutePermission $routePermission)
    {
        return $routePermission;
    }

    /**
     * Update the specified route permission.
     */
    public function update(Request $request, RoutePermission $routePermission)
    {
        $validated = $request->validate([
            'route_path' => 'sometimes|string|max:255',
            'route_method' => 'sometimes|string|in:GET,POST,PUT,PATCH,DELETE',
            'description' => 'nullable|string',
            'allowed_roles' => 'sometimes|array',
            'allowed_roles.*' => 'string|exists:roles,name',
            'is_active' => 'boolean',
        ]);

        $routePermission->update($validated);

        return response()->json($routePermission);
    }

    /**
     * Remove the specified route permission.
     */
    public function destroy(RoutePermission $routePermission)
    {
        $routePermission->delete();
        return response()->json(['message' => 'Route permission deleted']);
    }

    /**
     * Check if current user can access a specific route
     */
    public function check(Request $request)
    {
        $validated = $request->validate([
            'route_path' => 'required|string',
            'route_method' => 'string|in:GET,POST,PUT,PATCH,DELETE',
        ]);

        $method = $validated['route_method'] ?? 'GET';
        $routePermission = RoutePermission::findByRoute($validated['route_path'], $method);

        // If no route permission exists, allow access
        if (!$routePermission) {
            return response()->json([
                'allowed' => true,
                'message' => 'No restrictions for this route'
            ]);
        }

        $user = $request->user();
        $canAccess = $routePermission->canAccess($user);

        return response()->json([
            'allowed' => $canAccess,
            'allowed_roles' => $routePermission->allowed_roles,
            'user_roles' => $user->roles->pluck('name'),
            'message' => $canAccess ? 'Access granted' : 'Access denied'
        ]);
    }

    /**
     * Get all routes that current user can access
     */
    public function accessible(Request $request)
    {
        $user = $request->user();
        $userRoles = $user->roles->pluck('name')->toArray();

        $accessibleRoutes = RoutePermission::where('is_active', true)
            ->get()
            ->filter(function($permission) use ($userRoles) {
                return empty($permission->allowed_roles) || 
                       !empty(array_intersect($userRoles, $permission->allowed_roles));
            })
            ->map(function($permission) {
                return [
                    'route_path' => $permission->route_path,
                    'route_method' => $permission->route_method,
                ];
            });

        return response()->json($accessibleRoutes);
    }
    /**
     * Get all active route permission rules for frontend validation
     */
    public function rules()
    {
        $rules = RoutePermission::where('is_active', true)
            ->get()
            ->map(function($permission) {
                return [
                    'route_path' => $permission->route_path,
                    'route_method' => $permission->route_method,
                    'allowed_roles' => $permission->allowed_roles,
                ];
            });

        return response()->json($rules);
    }
}
