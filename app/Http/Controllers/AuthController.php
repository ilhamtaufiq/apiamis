<?php

namespace App\Http\Controllers;

use App\Models\AuditLog;
use App\Models\User;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\Hash;
use Illuminate\Support\Str;
use Illuminate\Validation\ValidationException;
use App\Http\Resources\UserResource;
use App\Services\MaintenanceModeService;
use App\Services\UserPresenceService;
use Laravel\Socialite\Facades\Socialite;

class AuthController extends Controller
{
    /**
     * Get Google OAuth redirect URL
     * 
     * @return \Illuminate\Http\JsonResponse
     */
    public function redirectToGoogle(Request $request)
    {
        /** @var \Laravel\Socialite\Two\AbstractProvider $driver */
        $driver = Socialite::driver('google');
        $platform = $request->query('platform') === 'mobile' ? 'mobile' : 'web';
        $callbackUrl = $this->normalizeOAuthCallbackUrl($request->query('callback_url'));

        $stateToken = Str::random(40);
        Cache::put("oauth_state:{$stateToken}", [
            'platform' => $platform,
            'callback_url' => $callbackUrl,
        ], now()->addMinutes(10));

        // Add gender scope (People API)
        $url = $driver->scopes(['https://www.googleapis.com/auth/user.gender.read'])
                      ->stateless()
                      ->with(['state' => $stateToken])
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
        $oauthCallbackBase = $this->resolveOAuthCallbackBase($request);
        
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
            
            $maintenance = app(MaintenanceModeService::class);
            if ($maintenance->isEnabled() && ! $maintenance->allowsUser($user)) {
                return redirect()->away(
                    $oauthCallbackBase . '#error=' . rawurlencode('Aplikasi sedang maintenance. Login ditutup sementara.')
                );
            }

            $user->load('roles', 'permissions');
            $token = $user->createToken('auth-token')->plainTextToken;
            
            // Token in URL fragment — not sent to server logs or Referer headers.
            return redirect()->away(
                $oauthCallbackBase . '#token=' . rawurlencode($token)
            );
        } catch (\Exception $e) {
            \Illuminate\Support\Facades\Log::warning('Google OAuth failed', [
                'message' => $e->getMessage(),
            ]);

            return redirect()->away(
                $oauthCallbackBase . '#error=' . rawurlencode('Google authentication failed. Please try again.')
            );
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

        $maintenance = app(MaintenanceModeService::class);
        if ($maintenance->isEnabled() && ! $maintenance->allowsUser($user)) {
            return response()->json([
                'message' => 'Aplikasi sedang maintenance. Login ditutup sementara.',
                'code' => 'MAINTENANCE_MODE',
                'maintenance' => true,
            ], 503);
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
        $user = $request->user();
        app(UserPresenceService::class)->remove($user);

        // Revoke current token
        $user->currentAccessToken()->delete();

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
     * Self-service profile update — semua user pada datanya sendiri,
     * tidak lewat PUT /users/{id} yang admin-only.
     */
    public function updateProfile(Request $request)
    {
        $validated = $request->validate([
            'name' => 'sometimes|required|string|max:255',
            'email' => 'sometimes|required|email|unique:users,email,'.$request->user()->id,
            'password' => 'nullable|string|min:6',
            'nip' => 'nullable|string|max:50',
            'jabatan' => 'nullable|string|max:255',
            'gender' => 'nullable|string|in:male,female,other',
            'avatar' => 'nullable|string|max:2048',
        ]);

        $user = $request->user();

        if (isset($validated['name'])) $user->name = $validated['name'];
        if (isset($validated['email'])) $user->email = $validated['email'];
        if (!empty($validated['password'])) $user->password = bcrypt($validated['password']);
        if (array_key_exists('nip', $validated)) $user->nip = $validated['nip'];
        if (array_key_exists('jabatan', $validated)) $user->jabatan = $validated['jabatan'];
        if (array_key_exists('gender', $validated)) $user->gender = $validated['gender'];
        if (array_key_exists('avatar', $validated)) $user->avatar = $validated['avatar'];
        $user->save();

        return new UserResource($user->fresh()->load('roles', 'permissions'));
    }

    /** Upload avatar file — disimpan via Spatie media (collection "avatar"). */
    public function uploadAvatar(Request $request)
    {
        $request->validate([
            'avatar' => 'required|image|mimes:jpg,jpeg,png,webp,gif|max:5120',
        ]);

        $user = $request->user();
        $user->clearMediaCollection('avatar');
        $user->addMediaFromRequest('avatar')->toMediaCollection('avatar');

        return new UserResource($user->fresh()->load('roles', 'permissions'));
    }

    public function deleteAvatar(Request $request)
    {
        $user = $request->user();
        $user->clearMediaCollection('avatar');

        return new UserResource($user->fresh()->load('roles', 'permissions'));
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

        AuditLog::create([
            'user_id' => $request->user()->id,
            'event' => 'impersonation_started',
            'auditable_type' => User::class,
            'auditable_id' => $user->id,
            'old_values' => null,
            'new_values' => [
                'impersonator_id' => $request->user()->id,
                'impersonator_email' => $request->user()->email,
                'target_user_id' => $user->id,
                'target_user_email' => $user->email,
            ],
            'url' => $request->fullUrl(),
            'ip_address' => $request->ip(),
            'user_agent' => (string) $request->userAgent(),
        ]);

        return response()->json([
            'user' => new UserResource($user),
            'token' => $token,
            'message' => "Now impersonating {$user->name}"
        ]);
    }

    public function createHandoff(Request $request)
    {
        $token = $request->user()->currentAccessToken();
        if (! $token) {
            return response()->json(['message' => 'Unauthenticated.'], 401);
        }

        $plainToken = $request->bearerToken();
        if (! $plainToken) {
            return response()->json(['message' => 'Bearer token required.'], 400);
        }

        $code = Str::random(48);
        Cache::put($this->handoffCacheKey($code), [
            'token' => $plainToken,
            'user_id' => $request->user()->id,
        ], now()->addMinute());

        return response()->json([
            'code' => $code,
            'expires_in' => 60,
        ]);
    }

    public function exchangeHandoff(Request $request)
    {
        $request->validate([
            'code' => 'required|string|size:48',
        ]);

        $payload = Cache::pull($this->handoffCacheKey($request->input('code')));
        if (! $payload || empty($payload['token'])) {
            return response()->json(['message' => 'Handoff code invalid or expired.'], 410);
        }

        $user = User::find($payload['user_id'] ?? null);
        if (! $user) {
            return response()->json(['message' => 'Handoff code invalid or expired.'], 410);
        }

        $user->load('roles', 'permissions');

        return response()->json([
            'user' => new UserResource($user),
            'token' => $payload['token'],
        ]);
    }

    private function handoffCacheKey(string $code): string
    {
        return 'auth_handoff:' . $code;
    }

    private function resolveOAuthCallbackBase(Request $request): string
    {
        $frontendUrl = env('FRONTEND_URL', 'http://arumanis.test');
        $mobileCallbackBase = rtrim((string) env('MOBILE_OAUTH_CALLBACK_URL', 'pengawas://oauth-callback'), '/');
        $stateToken = (string) $request->query('state', '');
        $oauthState = $stateToken !== '' ? Cache::pull("oauth_state:{$stateToken}") : null;

        if (is_array($oauthState)) {
            $platform = ($oauthState['platform'] ?? 'web') === 'mobile' ? 'mobile' : 'web';
            $callbackOverride = $this->normalizeOAuthCallbackUrl($oauthState['callback_url'] ?? null);

            if ($platform === 'mobile') {
                return $callbackOverride ?: $mobileCallbackBase;
            }

            return rtrim($frontendUrl, '/') . '/oauth-callback';
        }

        // Legacy fallback: plain "mobile" state from older clients.
        if ($stateToken === 'mobile') {
            return $mobileCallbackBase;
        }

        return rtrim($frontendUrl, '/') . '/oauth-callback';
    }

    private function normalizeOAuthCallbackUrl(mixed $value): ?string
    {
        if (! is_string($value)) {
            return null;
        }

        $trimmed = trim($value);
        if ($trimmed === '') {
            return null;
        }

        if (str_starts_with($trimmed, 'pengawas://')) {
            return rtrim($trimmed, '/');
        }

        if (! filter_var($trimmed, FILTER_VALIDATE_URL)) {
            return null;
        }

        $parts = parse_url($trimmed);
        if (! is_array($parts) || empty($parts['scheme']) || empty($parts['host'])) {
            return null;
        }

        if (! in_array(strtolower((string) $parts['scheme']), ['http', 'https'], true)) {
            return null;
        }

        return rtrim($trimmed, '/');
    }
}
