# Google OAuth Authentication Implementation

## Overview
Implement Google OAuth authentication and user registration for APIAMIS backend. This allows users to authenticate via their Google account as an alternative to email/password login.

## User Review Required

> [!IMPORTANT]
> **Google Cloud Project Setup Required**
> Before implementation, you need to create a Google Cloud Project and obtain OAuth credentials:
> 1. Go to [Google Cloud Console](https://console.cloud.google.com)
> 2. Create a new project or select existing one
> 3. Enable "Google+ API" or "Google People API"
> 4. Configure OAuth Consent Screen (External)
> 5. Create OAuth 2.0 Client ID (Web application)
> 6. Add authorized redirect URIs:
>    - Development: `http://apiamis.test/api/auth/google/callback`
>    - Production: `https://apiamis.ilham.wtf/api/auth/google/callback`

> [!CAUTION]
> **New Users Role Assignment**
> New users registered via Google OAuth will be assigned a default role. Please confirm:
> - What role should new Google OAuth users receive? (e.g., `user`, `viewer`, or no role)
> - Should there be email domain restrictions? (e.g., only `@ilham.wtf` emails)

---

## Proposed Changes

### Package Installation

Install Laravel Socialite for OAuth provider support:

```bash
composer require laravel/socialite
```

---

### Configuration

#### [MODIFY] [.env](file:///c:/laragon/www/apiamis/.env)
Add Google OAuth credentials:
```env
GOOGLE_CLIENT_ID=your-google-client-id
GOOGLE_CLIENT_SECRET=your-google-client-secret
GOOGLE_REDIRECT_URI="${APP_URL}/api/auth/google/callback"
```

#### [MODIFY] [config/services.php](file:///c:/laragon/www/apiamis/config/services.php)
Add Google OAuth configuration:
```php
'google' => [
    'client_id' => env('GOOGLE_CLIENT_ID'),
    'client_secret' => env('GOOGLE_CLIENT_SECRET'),
    'redirect' => env('GOOGLE_REDIRECT_URI'),
],
```

---

### Database

#### [NEW] [database/migrations/xxxx_add_google_oauth_fields_to_users_table.php](file:///c:/laragon/www/apiamis/database/migrations)
Add columns to users table:
- `google_id` - Google unique identifier
- `avatar` - Google profile picture URL
- `password` - Make nullable for OAuth-only users

```php
Schema::table('users', function (Blueprint $table) {
    $table->string('google_id')->nullable()->unique()->after('id');
    $table->string('avatar')->nullable()->after('email');
});

// Make password nullable for OAuth users
Schema::table('users', function (Blueprint $table) {
    $table->string('password')->nullable()->change();
});
```

---

### Model

#### [MODIFY] [User.php](file:///c:/laragon/www/apiamis/app/Models/User.php)
Update fillable properties:
```php
protected $fillable = [
    'name',
    'email',
    'password',
    'google_id',
    'avatar',
];
```

---

### Controller

#### [MODIFY] [AuthController.php](file:///c:/laragon/www/apiamis/app/Http/Controllers/AuthController.php)
Add Google OAuth methods:

| Method | Description |
|--------|-------------|
| `redirectToGoogle()` | Returns Google OAuth URL for frontend to redirect |
| `handleGoogleCallback()` | Handles OAuth callback, creates/updates user, returns Sanctum token |

```php
use Laravel\Socialite\Facades\Socialite;

public function redirectToGoogle()
{
    $url = Socialite::driver('google')->stateless()->redirect()->getTargetUrl();
    return response()->json(['url' => $url]);
}

public function handleGoogleCallback(Request $request)
{
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
    
    // Assign default role to new users
    if ($user->wasRecentlyCreated) {
        $user->assignRole('user'); // or appropriate default role
    }
    
    $user->load('roles', 'permissions');
    $token = $user->createToken('auth-token')->plainTextToken;
    
    return response()->json([
        'user' => new UserResource($user),
        'token' => $token,
    ]);
}
```

---

### Routes

#### [MODIFY] [api.php](file:///c:/laragon/www/apiamis/routes/api.php)
Add Google OAuth routes (public, before auth middleware):

```php
// Google OAuth Routes
Route::get('auth/google', [AuthController::class, 'redirectToGoogle']);
Route::get('auth/google/callback', [AuthController::class, 'handleGoogleCallback']);
```

---

## API Endpoints Summary

| Method | Endpoint | Description | Auth |
|--------|----------|-------------|------|
| GET | `/api/auth/google` | Get Google OAuth redirect URL | Public |
| GET | `/api/auth/google/callback` | Handle Google OAuth callback | Public |
| POST | `/api/auth/login` | Traditional email/password login | Public |
| POST | `/api/auth/logout` | Logout user | Protected |
| GET | `/api/auth/me` | Get current user | Protected |

---

## Verification Plan

### Automated Tests
1. Run existing tests to ensure no regression:
   ```bash
   php artisan test
   ```

2. Test database migration:
   ```bash
   php artisan migrate
   ```

### Manual Verification
1. Call `GET /api/auth/google` and verify it returns a valid Google OAuth URL
2. Complete OAuth flow in browser and verify callback returns user + token
3. Verify existing email/password login still works
4. Verify logout functionality works for both login methods
5. Test with new user (should create account)
6. Test with existing user (should link Google account)

---

## Frontend Integration Notes

The frontend (ARUMANIS) will need to:
1. Add "Login with Google" button
2. Call `GET /api/auth/google` to get OAuth URL
3. Redirect user to Google OAuth URL
4. Handle callback by extracting token from response
5. Store token and redirect to dashboard

```typescript
// Example frontend flow
const handleGoogleLogin = async () => {
  const response = await api.get('/auth/google');
  window.location.href = response.data.url;
};

// In callback page
const handleCallback = async () => {
  // Google will redirect to: /api/auth/google/callback?code=...
  // Backend handles this and returns token
  // Frontend should either:
  // 1. Use cookie-based auth (SPA)
  // 2. Or implement a frontend callback handler
};
```
