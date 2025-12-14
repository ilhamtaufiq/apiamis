<?php

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;
use Symfony\Component\HttpFoundation\Response;
use App\Models\RoutePermission;

class CheckRoutePermission
{
    /**
     * Handle an incoming request.
     *
     * @param  \Closure(\Illuminate\Http\Request): (\Symfony\Component\HttpFoundation\Response)  $next
     */
    public function handle(Request $request, Closure $next): Response
    {
        $user = $request->user();
        
        // If no user is authenticated, let the auth middleware handle it
        if (!$user) {
            return $next($request);
        }

        // Admin bypass - admin selalu dapat akses semua route
        if ($user->hasRole('admin')) {
            return $next($request);
        }

        // Get current route path and method
        $path = $request->path();
        $method = $request->method();
        
        // Normalize path: remove 'api/' prefix if present and ensure leading slash
        if (str_starts_with($path, 'api/')) {
            $path = '/' . substr($path, 4);
        } elseif (!str_starts_with($path, '/')) {
            $path = '/' . $path;
        }

        // Whitelist system routes that must always be accessible
        $whitelistedRoutes = [
            '/auth/me',
            '/auth/logout',
            '/menu-permissions/user/menus',
            '/route-permissions/rules',
            '/route-permissions/user/accessible',
            '/dashboard/stats',
            '/app-settings',
        ];
        
        // Check exact match
        if (in_array($path, $whitelistedRoutes)) {
            return $next($request);
        }

        // Find route permission for this path and method
        $routePermission = RoutePermission::findByRoute($path, $method);

        // If no route permission exists, deny access (default deny for non-admin)
        if (!$routePermission) {
            return response()->json([
                'message' => 'Akses ditolak. Route ini tidak memiliki konfigurasi permission.',
                'route' => "$method $path",
                'your_roles' => $user->roles->pluck('name'),
            ], 403);
        }

        // Check if user can access this route
        if (!$routePermission->canAccess($user)) {
            return response()->json([
                'message' => 'Akses ditolak. Anda tidak memiliki permission untuk mengakses route ini.',
                'required_roles' => $routePermission->allowed_roles,
                'your_roles' => $user->roles->pluck('name'),
            ], 403);
        }

        return $next($request);
    }
}
