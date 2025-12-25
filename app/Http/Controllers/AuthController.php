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
        $url = Socialite::driver('google')->stateless()->redirect()->getTargetUrl();
        return response()->json(['url' => $url]);
    }

    /**
     * Handle Google OAuth callback
     * 
     * @param Request $request
     * @return \Illuminate\Http\JsonResponse
     */
    public function handleGoogleCallback(Request $request)
    {
        // Get frontend URL from environment or use default
        $frontendUrl = env('FRONTEND_URL', 'http://arumanis.test');
        
        try {
            $googleUser = Socialite::driver('google')->stateless()->user();
            
            $user = User::updateOrCreate(
                ['email' => $googleUser->getEmail()],
                [
                    'name' => $googleUser->getName(),
                    'google_id' => $googleUser->getId(),
                    'avatar' => $googleUser->getAvatar(),
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
     * Handle user login
     * 
     * @param Request $request
     * @return \Illuminate\Http\JsonResponse
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
     * Handle user logout
     * 
     * @param Request $request
     * @return \Illuminate\Http\JsonResponse
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
     * Get authenticated user
     * 
     * @param Request $request
     * @return UserResource
     */
    public function me(Request $request)
    {
        return new UserResource($request->user()->load('roles', 'permissions'));
    }
}
