<?php

namespace App\Http\Middleware;

use App\Services\MaintenanceModeService;
use Closure;
use Illuminate\Http\Request;
use Symfony\Component\HttpFoundation\Response;

class EnsureNotInMaintenance
{
    public function __construct(
        private readonly MaintenanceModeService $maintenance,
    ) {}

    public function handle(Request $request, Closure $next): Response
    {
        if (! $this->maintenance->isEnabled()) {
            return $next($request);
        }

        $path = $request->path();
        if ($this->maintenance->isExemptPath($path)) {
            return $next($request);
        }

        // Allow bypass users (Bearer / Sanctum) full API access during maintenance.
        $user = $this->maintenance->resolveUser($request);
        if ($this->maintenance->allowsUser($user)) {
            return $next($request);
        }

        // Authenticated non-bypass: still blocked.
        // Unauthenticated: blocked except exempt paths above.
        return response()->json([
            'message' => 'Aplikasi sedang dalam mode maintenance. Coba lagi nanti.',
            'code' => 'MAINTENANCE_MODE',
            'maintenance' => true,
        ], 503);
    }
}
