<?php

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;
use Symfony\Component\HttpFoundation\Response;
use App\Models\RoutePermission;

class CheckRoutePermission
{
    /**
     * Routes that are admin-only by default (if no specific rule exists in database)
     * These routes will be denied for non-admin users unless explicitly allowed
     * 
     * NOTE: Do NOT include data-fetching routes here (e.g., /penyedia, /kegiatan, etc.)
     * as they are needed by components to load dropdown data, etc.
     * Only include routes that are truly for admin management pages.
     */
    private const ADMIN_ONLY_ROUTES = [
        '/users',
        '/roles',
        '/permissions',
        '/route-permissions',
        '/menu-permissions',
    ];

    /**
     * Check if a route is in the admin-only list
     */
    private function isAdminOnlyRoute(string $path): bool
    {
        // Check exact match first
        if (in_array($path, self::ADMIN_ONLY_ROUTES)) {
            return true;
        }

        // Check if the base path (first segment) is admin-only
        // e.g., /kegiatan/123 should match /kegiatan
        $segments = array_filter(explode('/', $path));
        if (!empty($segments)) {
            $basePath = '/' . reset($segments);
            return in_array($basePath, self::ADMIN_ONLY_ROUTES);
        }

        return false;
    }

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

        // If a rule exists, check if user can access
        if ($routePermission) {
            if (!$routePermission->canAccess($user)) {
                return response()->json([
                    'message' => 'Akses ditolak. Anda tidak memiliki permission untuk mengakses route ini.',
                    'required_roles' => $routePermission->allowed_roles,
                    'your_roles' => $user->roles->pluck('name'),
                ], 403);
            }
            return $next($request);
        }

        // No rule found - check if this is an admin-only route
        if ($this->isAdminOnlyRoute($path)) {
            return response()->json([
                'message' => 'Akses ditolak. Route ini hanya dapat diakses oleh admin.',
                'route' => "$method $path",
                'your_roles' => $user->roles->pluck('name'),
            ], 403);
        }

        // For non-admin-only routes without rules, allow access
        return $next($request);
    }
}
