<?php

namespace App\Http\Controllers;

use Spatie\Permission\Models\Permission;
use Illuminate\Http\Request;

class PermissionController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/permissions",
     *     summary="List all permissions",
     *     tags={"Access Control (Standard Permissions)"},
     *     @OA\Parameter(name="search", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = Permission::query();

        // Add search functionality
        if ($request->has('search') && $request->search) {
            $query->where('name', 'like', '%' . $request->search . '%');
        }

        return $query->paginate(15);
    }

    /**
     * @OA\Post(
     *     path="/api/permissions",
     *     summary="Create new permission",
     *     tags={"Access Control (Standard Permissions)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"name"},
     *             @OA\Property(property="name", type="string")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'name' => 'required|string|unique:permissions,name',
        ]);

        $permission = Permission::firstOrCreate([
            'name' => $validated['name'],
            'guard_name' => 'web'
        ]);

        return response()->json($permission, 201);
    }

    /**
     * @OA\Get(
     *     path="/api/permissions/{id}",
     *     summary="Get permission detail",
     *     tags={"Access Control (Standard Permissions)"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Permission $permission)
    {
        return $permission;
    }

    /**
     * @OA\Put(
     *     path="/api/permissions/{id}",
     *     summary="Update permission",
     *     tags={"Access Control (Standard Permissions)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, Permission $permission)
    {
        $validated = $request->validate([
            'name' => 'required|string|unique:permissions,name,' . $permission->id,
        ]);

        $permission->name = $validated['name'];
        $permission->save();

        return $permission;
    }

    /**
     * @OA\Delete(
     *     path="/api/permissions/{id}",
     *     summary="Delete permission",
     *     tags={"Access Control (Standard Permissions)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Permission $permission)
    {
        $permission->delete();
        return response()->json(['message' => 'Permission deleted']);
    }
}
