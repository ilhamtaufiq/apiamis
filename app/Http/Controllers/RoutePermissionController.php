<?php

namespace App\Http\Controllers;

use App\Models\RoutePermission;
use Illuminate\Http\Request;

class RoutePermissionController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/route-permissions",
     *     summary="List all route permissions",
     *     tags={"Access Control (Route)"},
     *     @OA\Parameter(name="search", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Parameter(name="method", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
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
        if ($request->has('method') && $request->input('method')) {
            $query->where('route_method', strtoupper($request->input('method')));
        }

        // Filter by active status
        if ($request->has('is_active')) {
            $query->where('is_active', $request->boolean('is_active'));
        }

        if ($request->filled('per_page') && $request->per_page == -1) {
            return $query->orderBy('route_path')->get();
        }

        return $query->orderBy('route_path')->paginate($request->input('per_page', 15));
    }

    /**
     * @OA\Post(
     *     path="/api/route-permissions",
     *     summary="Create new route permission",
     *     tags={"Access Control (Route)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"route_path", "route_method", "allowed_roles"},
     *             @OA\Property(property="route_path", type="string"),
     *             @OA\Property(property="route_method", type="string", enum={"GET","POST","PUT","PATCH","DELETE"}),
     *             @OA\Property(property="allowed_roles", type="array", @OA\Items(type="string"))
     *         )
     *     ),
     *     @OA\Response(response=201, description="Created")
     * )
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
     * @OA\Get(
     *     path="/api/route-permissions/{id}",
     *     summary="Get route permission detail",
     *     tags={"Access Control (Route)"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(RoutePermission $routePermission)
    {
        return $routePermission;
    }

    /**
     * @OA\Put(
     *     path="/api/route-permissions/{id}",
     *     summary="Update route permission",
     *     tags={"Access Control (Route)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
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
     * @OA\Delete(
     *     path="/api/route-permissions/{id}",
     *     summary="Delete route permission",
     *     tags={"Access Control (Route)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(RoutePermission $routePermission)
    {
        $routePermission->delete();
        return response()->json(['message' => 'Route permission deleted']);
    }

    /**
     * @OA\Post(
     *     path="/api/route-permissions/check",
     *     summary="Check access for a route",
     *     tags={"Access Control (Route)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"route_path"},
     *             @OA\Property(property="route_path", type="string"),
     *             @OA\Property(property="route_method", type="string")
     *         )
     *     ),
     *     @OA\Response(response=200, description="Check result")
     * )
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
     * @OA\Get(
     *     path="/api/route-permissions/accessible",
     *     summary="Get all accessible routes for current user",
     *     tags={"Access Control (Route)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Response(response=200, description="Successful operation")
     * )
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
     * @OA\Get(
     *     path="/api/route-permissions/rules",
     *     summary="Get all active route permission rules",
     *     tags={"Access Control (Route)"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
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
