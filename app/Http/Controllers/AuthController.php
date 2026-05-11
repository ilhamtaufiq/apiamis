<?php

namespace App\Http\Controllers;

use App\Models\User;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Hash;
use Illuminate\Validation\ValidationException;
use App\Http\Resources\UserResource;
use Laravel\Socialite\Facades\Socialite;

class AuthController extends Controller
{
    /**
     * Get Google OAuth redirect URL
     * 
     * @return \Illuminate\Http\JsonResponse
     */
    public function redirectToGoogle()
    {
        /** @var \Laravel\Socialite\Two\AbstractProvider $driver */
        $driver = Socialite::driver('google');
        // Add gender scope (People API)
        $url = $driver->scopes(['https://www.googleapis.com/auth/user.gender.read'])
                      ->stateless()
                      ->redirect()
                      ->getTargetUrl();
        return response()->json(['url' => $url]);
    }

    /**
     * Handle Google OAuth callback
     * 
     * @param Request $request
     * @return \Illuminate\Http\RedirectResponse
     */
    public function handleGoogleCallback(Request $request)
    {
        // Get frontend URL from environment or use default
        $frontendUrl = env('FRONTEND_URL', 'http://arumanis.test');
        
        try {
            /** @var \Laravel\Socialite\Two\AbstractProvider $driver */
            $driver = Socialite::driver('google');
            $googleUser = $driver->stateless()->user();
            
            // Extract gender if available from raw data (Google People API format)
            $gender = null;
            $rawUser = $googleUser->getRaw();
            if (isset($rawUser['genders'][0]['value'])) {
                $gender = $rawUser['genders'][0]['value']; // 'male', 'female', etc.
            }

            $user = User::updateOrCreate(
                ['email' => $googleUser->getEmail()],
                [
                    'name' => $googleUser->getName(),
                    'google_id' => $googleUser->getId(),
                    'avatar' => $googleUser->getAvatar(),
                    'gender' => $gender,
                    'email_verified_at' => now(),
                ]
            );
            
            // Assign default role to new users safely
            if ($user->wasRecentlyCreated) {
                try {
                    // Try to find role with 'web' guard specifically
                    $defaultRole = \Spatie\Permission\Models\Role::where('name', 'user')
                        ->where('guard_name', 'web')
                        ->first();
                    
                    if ($defaultRole) {
                        $user->assignRole($defaultRole);
                    }
                } catch (\Exception $e) {
                    // Log error but don't fail the entire login
                    \Illuminate\Support\Facades\Log::warning('Failed to assign default role during Google OAuth: ' . $e->getMessage());
                }
            }
            
            $user->load('roles', 'permissions');
            $token = $user->createToken('auth-token')->plainTextToken;
            
            // Redirect to frontend with token
            return redirect()->away(rtrim($frontendUrl, '/') . '/oauth-callback?token=' . $token);
        } catch (\Exception $e) {
            // Redirect to frontend with error
            return redirect()->away(rtrim($frontendUrl, '/') . '/oauth-callback?error=' . urlencode('Google authentication failed: ' . $e->getMessage()));
        }
    }

    /**
     * @OA\Post(
     *     path="/api/auth/login",
     *     summary="User login",
     *     description="Authenticate user with email and password to receive a Bearer token",
     *     tags={"Authentication"},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"email", "password"},
     *             @OA\Property(property="email", type="string", format="email", example="admin@example.com"),
     *             @OA\Property(property="password", type="string", format="password", example="password")
     *         )
     *     ),
     *     @OA\Response(
     *         response=200,
     *         description="Login successful",
     *         @OA\JsonContent(
     *             @OA\Property(property="user", type="object"),
     *             @OA\Property(property="token", type="string", example="1|abc123yourtoken")
     *         )
     *     ),
     *     @OA\Response(response=422, description="Validation error")
     * )
     */
    public function login(Request $request)
    {
        $request->validate([
            'email' => 'required|email',
            'password' => 'required|string',
        ]);

        $user = User::where('email', $request->email)->first();

        if (!$user || !Hash::check($request->password, $user->password)) {
            throw ValidationException::withMessages([
                'email' => ['The provided credentials are incorrect.'],
            ]);
        }

        // Load roles and permissions
        $user->load('roles', 'permissions');

        // Create token
        $token = $user->createToken('auth-token')->plainTextToken;

        return response()->json([
            'user' => new UserResource($user),
            'token' => $token,
        ]);
    }

    /**
     * @OA\Post(
     *     path="/api/auth/logout",
     *     summary="User logout",
     *     description="Revoke current authenticated user's token",
     *     tags={"Authentication"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Response(
     *         response=200,
     *         description="Logged out successfully",
     *         @OA\JsonContent(
     *             @OA\Property(property="message", type="string", example="Logged out successfully")
     *         )
     *     ),
     *     @OA\Response(response=401, description="Unauthenticated")
     * )
     */
    public function logout(Request $request)
    {
        // Revoke current token
        $request->user()->currentAccessToken()->delete();

        return response()->json([
            'message' => 'Logged out successfully'
        ]);
    }

    /**
     * @OA\Get(
     *     path="/api/auth/me",
     *     summary="Get authenticated user detail",
     *     description="Returns detailed information about the currently authenticated user including roles and permissions",
     *     tags={"Authentication"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Response(
     *         response=200,
     *         description="User profile retrieved successfully"
     *     ),
     *     @OA\Response(response=401, description="Unauthenticated")
     * )
     */
    public function me(Request $request)
    {
        return new UserResource($request->user()->load('roles', 'permissions'));
    }

    /**
     * @OA\Post(
     *     path="/api/auth/impersonate/{user}",
     *     summary="Impersonate a user (Admin only)",
     *     description="Generate a session token for another user to act on their behalf",
     *     tags={"Authentication"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(
     *         name="user",
     *         in="path",
     *         required=true,
     *         description="ID of the user to impersonate",
     *         @OA\Schema(type="integer")
     *     ),
     *     @OA\Response(
     *         response=200,
     *         description="Impersonation token created",
     *         @OA\JsonContent(
     *             @OA\Property(property="user", type="object"),
     *             @OA\Property(property="token", type="string"),
     *             @OA\Property(property="message", type="string")
     *         )
     *     ),
     *     @OA\Response(response=403, description="Forbidden/Unauthorized"),
     *     @OA\Response(response=422, description="Cannot impersonate yourself")
     * )
     */
    public function impersonate(Request $request, User $user)
    {
        // Safety check (already handled by middleware but good to have)
        if (!$request->user()->hasRole('admin')) {
            return response()->json(['message' => 'Unauthorized'], 403);
        }

        // Prevent self-impersonation to avoid confusion
        if ($request->user()->id === $user->id) {
            return response()->json(['message' => 'Cannot impersonate yourself'], 422);
        }

        // Load roles and permissions
        $user->load('roles', 'permissions');

        // Create token for the target user
        $token = $user->createToken('impersonation-token')->plainTextToken;

        return response()->json([
            'user' => new UserResource($user),
            'token' => $token,
            'message' => "Now impersonating {$user->name}"
        ]);
    }
}
