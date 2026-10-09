<?php

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;

/**
 * Menerima token Sanctum dari cookie httpOnly `arumanis_token` (alur SSO browser).
 *
 * Hanya dipasang di grup middleware `api`. Bila request membawa header
 * `Authorization` (apa pun isinya), header itu dipakai apa adanya dan cookie
 * diabaikan. Bila header tidak ada dan cookie ada, token dari cookie disalin
 * ke `Authorization: Bearer` supaya `auth:sanctum` bekerja seperti biasa.
 *
 * Proteksi CSRF tambahan: request cookie yang mengubah state (POST, PUT, PATCH,
 * DELETE) hanya diterima bila membawa salah satu header `X-Arumanis-App`,
 * `X-Requested-With`, atau `Content-Type: application/json`.
 */
class AcceptAuthCookie
{
    private const STATE_CHANGING_METHODS = ['POST', 'PUT', 'PATCH', 'DELETE'];

    public function handle(Request $request, Closure $next)
    {
        // Header Authorization yang sudah ada selalu didahulukan (perilaku bearer lama).
        if ($request->headers->has('Authorization')) {
            return $next($request);
        }

        $token = $request->cookie(config('sanctum.auth_cookie.name', 'arumanis_token'));
        if (! is_string($token) || $token === '') {
            return $next($request);
        }

        if (in_array($request->method(), self::STATE_CHANGING_METHODS, true) && ! $this->hasCsrfHeader($request)) {
            return response()->json(['message' => 'Unauthenticated.'], 401);
        }

        $request->headers->set('Authorization', 'Bearer '.$token);

        return $next($request);
    }

    /**
     * Cek header yang hanya bisa dikirim lewat fetch/XHR, bukan form lintas situs.
     */
    private function hasCsrfHeader(Request $request): bool
    {
        if ($request->headers->has('X-Arumanis-App') || $request->headers->has('X-Requested-With')) {
            return true;
        }

        return str_contains(strtolower((string) $request->header('Content-Type')), 'application/json');
    }
}
