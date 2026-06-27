<?php

namespace App\Http\Middleware;

use App\Models\User;
use Closure;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Auth;
use Laravel\Sanctum\PersonalAccessToken;
use Symfony\Component\HttpFoundation\Response;

class EnsureSwaggerAdminAccess
{
    public function handle(Request $request, Closure $next): Response
    {
        $accessToken = $request->query('access_token');
        if (is_string($accessToken) && $accessToken !== '') {
            return $this->bootstrapSessionFromToken($request, $accessToken);
        }

        $user = $this->resolveUser($request);

        if (! $user) {
            return $this->deny($request, 'Unauthenticated.', 401);
        }

        if (! $user->hasRole('admin')) {
            return $this->deny($request, 'Akses ditolak. Dokumentasi API hanya untuk admin.', 403);
        }

        Auth::setUser($user);

        return $next($request);
    }

    private function bootstrapSessionFromToken(Request $request, string $plainTextToken): Response
    {
        $user = $this->userFromPlainTextToken($plainTextToken);

        if (! $user || ! $user->hasRole('admin')) {
            return $this->deny($request, 'Token tidak valid atau Anda bukan admin.', 403);
        }

        Auth::guard('web')->login($user);
        $request->session()->regenerate();

        $query = $request->except('access_token');
        $target = $request->url().(empty($query) ? '' : '?'.http_build_query($query));

        return redirect()->to($target);
    }

    private function resolveUser(Request $request): ?User
    {
        $sessionUser = Auth::guard('web')->user();
        if ($sessionUser instanceof User) {
            return $sessionUser;
        }

        $bearer = $request->bearerToken();
        if (is_string($bearer) && $bearer !== '') {
            return $this->userFromPlainTextToken($bearer);
        }

        return null;
    }

    private function userFromPlainTextToken(string $plainTextToken): ?User
    {
        $accessToken = PersonalAccessToken::findToken($plainTextToken);

        if (! $accessToken) {
            return null;
        }

        if ($accessToken->expires_at && $accessToken->expires_at->isPast()) {
            return null;
        }

        $user = $accessToken->tokenable;

        return $user instanceof User ? $user : null;
    }

    private function deny(Request $request, string $message, int $status): Response
    {
        if ($request->expectsJson()) {
            return response()->json(['message' => $message], $status);
        }

        return response()->view('swagger-denied', [
            'message' => $message,
            'status' => $status,
        ], $status);
    }
}