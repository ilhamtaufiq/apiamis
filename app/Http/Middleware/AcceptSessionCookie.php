<?php

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;

/**
 * Menerima sesi dari cookie `arumanis_session` (pengganti BFF).
 *
 * Cookie berisi token Sanctum `id|plain`. Bila request tidak membawa
 * `Authorization: Bearer`, cookie itu disalin ke header tersebut supaya
 * `auth:sanctum` di endpoint Laravel tetap bekerja.
 */
class AcceptSessionCookie
{
    public const COOKIE = 'arumanis_session';

    public function handle(Request $request, Closure $next)
    {
        if ($request->bearerToken() === null) {
            $token = $request->cookie(self::COOKIE);
            if (is_string($token) && $token !== '') {
                $request->headers->set('Authorization', 'Bearer '.$token);
            }
        }

        return $next($request);
    }
}
