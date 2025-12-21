# Google OAuth Implementation Tasks

## Status Legend
- `[ ]` Not started
- `[/]` In progress
- `[x]` Completed

---

## Prerequisites

- [ ] Create Google Cloud Project
- [ ] Configure OAuth Consent Screen
- [ ] Create OAuth 2.0 Client ID
- [ ] Obtain Client ID and Client Secret
- [ ] Add authorized redirect URIs

---

## Backend Implementation

### Phase 1: Setup & Configuration
- [ ] Install Laravel Socialite package
  ```bash
  composer require laravel/socialite
  ```
- [ ] Add Google OAuth credentials to `.env`
- [ ] Configure `config/services.php` with Google provider
- [ ] Update `.env.example` with new variables

### Phase 2: Database
- [ ] Create migration for OAuth fields on users table
  - [ ] Add `google_id` column (nullable, unique)
  - [ ] Add `avatar` column (nullable)
  - [ ] Make `password` nullable
- [ ] Run migration
- [ ] Update User model `$fillable` array

### Phase 3: Controller & Routes
- [ ] Add `redirectToGoogle()` method to `AuthController`
- [ ] Add `handleGoogleCallback()` method to `AuthController`
- [ ] Add OAuth routes to `api.php`
- [ ] Test callback URL accessibility

### Phase 4: User Registration Logic
- [ ] Implement `updateOrCreate` for Google users
- [ ] Assign default role to new OAuth users
- [ ] Handle existing users linking Google account
- [ ] Return Sanctum token after OAuth

---

## Testing

- [ ] Run existing test suite (regression check)
- [ ] Test Google OAuth redirect URL endpoint
- [ ] Test complete OAuth flow (manual)
- [ ] Verify email/password login still works
- [ ] Test new user registration via Google
- [ ] Test existing user login via Google
- [ ] Test logout works for both methods

---

## Documentation

- [ ] Update API documentation (Swagger)
- [ ] Document new environment variables
- [ ] Update README with Google OAuth setup instructions

---

## Frontend Integration (ARUMANIS)

> Note: These tasks are for the frontend project

- [ ] Add "Login with Google" button to login page
- [ ] Implement Google OAuth redirect logic
- [ ] Handle OAuth callback response
- [ ] Store token and update auth state
- [ ] Test full login flow end-to-end

---

## Deployment

- [ ] Add environment variables to production
- [ ] Update Google Cloud Console with production redirect URI
- [ ] Deploy and test in production environment
