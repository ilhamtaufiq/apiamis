<?php

namespace App\Http\Controllers;

use App\Models\User;
use Illuminate\Http\Request;
use Spatie\Permission\Models\Role;
use Spatie\Permission\Models\Permission;

class UserController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/users",
     *     summary="List all users",
     *     tags={"User Management"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index()
    {
        $users = User::with('roles', 'permissions')->paginate(15);
        return \App\Http\Resources\UserResource::collection($users);
    }

    /**
     * @OA\Post(
     *     path="/api/users",
     *     summary="Create new user",
     *     tags={"User Management"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"name", "email", "password"},
     *             @OA\Property(property="name", type="string"),
     *             @OA\Property(property="email", type="string", format="email"),
     *             @OA\Property(property="password", type="string", minLength=6)
     *         )
     *     ),
     *     @OA\Response(response=201, description="User created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'name' => 'required|string|max:255',
            'email' => 'required|email|unique:users,email',
            'password' => 'required|string|min:6',
            'nip' => 'nullable|string|max:50',
            'jabatan' => 'nullable|string|max:255',
            'roles' => 'sometimes|array',
            'permissions' => 'sometimes|array',
        ]);

        $user = User::create([
            'name' => $validated['name'],
            'email' => $validated['email'],
            'password' => bcrypt($validated['password']),
            'nip' => $validated['nip'] ?? null,
            'jabatan' => $validated['jabatan'] ?? null,
        ]);

        if (isset($validated['roles'])) {
            $user->syncRoles($validated['roles']);
        }

        if (isset($validated['permissions'])) {
            $user->syncPermissions($validated['permissions']);
        }

        return response()->json(new \App\Http\Resources\UserResource($user->load('roles', 'permissions')), 201);
    }

    /**
     * @OA\Get(
     *     path="/api/users/{id}",
     *     summary="Get user detail",
     *     tags={"User Management"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(User $user)
    {
        return new \App\Http\Resources\UserResource($user->load('roles', 'permissions'));
    }

    /**
     * @OA\Put(
     *     path="/api/users/{id}",
     *     summary="Update user",
     *     tags={"User Management"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="User updated")
     * )
     */
    public function update(Request $request, User $user)
    {
        $validated = $request->validate([
            'name' => 'sometimes|string|max:255',
            'email' => 'sometimes|email|unique:users,email,' . $user->id,
            'password' => 'nullable|string|min:6',
            'nip' => 'nullable|string|max:50',
            'jabatan' => 'nullable|string|max:255',
            'roles' => 'sometimes|array',
            'permissions' => 'sometimes|array',
        ]);

        if (isset($validated['name'])) $user->name = $validated['name'];
        if (isset($validated['email'])) $user->email = $validated['email'];
        if (!empty($validated['password'])) $user->password = bcrypt($validated['password']);
        if (array_key_exists('nip', $validated)) $user->nip = $validated['nip'];
        if (array_key_exists('jabatan', $validated)) $user->jabatan = $validated['jabatan'];
        $user->save();

        if (isset($validated['roles'])) {
            $user->syncRoles($validated['roles']);
        }

        if (isset($validated['permissions'])) {
            $user->syncPermissions($validated['permissions']);
        }

        return response()->json(new \App\Http\Resources\UserResource($user->load('roles', 'permissions')));
    }

    /**
     * @OA\Delete(
     *     path="/api/users/{id}",
     *     summary="Delete user",
     *     tags={"User Management"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="User deleted")
     * )
     */
    public function destroy(User $user)
    {
        $user->delete();
        return response()->json(['message' => 'User deleted']);
    }
}
