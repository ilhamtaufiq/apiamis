<?php

namespace App\Services;

use App\Models\RoutePermission;
use Illuminate\Support\Facades\Route;

class RoutePermissionSyncService
{
    /**
     * @return array{
     *     scanned: int,
     *     created: int,
     *     removed: int,
     *     prefix: string,
     *     default_role: string
     * }
     */
    public function sync(string $prefix = 'api', string $defaultRole = 'admin', bool $clean = false): array
    {
        $routes = Route::getRoutes();
        $processedRoutes = [];
        $newCount = 0;
        $totalCount = 0;
        $removedCount = 0;

        foreach ($routes as $route) {
            $uri = $route->uri();

            if ($prefix && ! str_starts_with($uri, $prefix)) {
                continue;
            }

            $cleanUri = $uri;
            if (str_starts_with($uri, 'api/')) {
                $cleanUri = substr($uri, 4);
            }

            $path = '/'.preg_replace('/\{([a-zA-Z0-9_?]+)\}/', ':$1', $cleanUri);
            $path = str_replace('?', '', $path);
            $path = preg_replace('/\/+/', '/', $path);
            $path = rtrim($path, '/');
            if ($path === '') {
                $path = '/';
            }

            $methods = array_filter($route->methods(), fn ($method) => $method !== 'HEAD');

            foreach ($methods as $method) {
                $processedRoutes[] = [
                    'path' => $path,
                    'method' => $method,
                ];

                $permission = RoutePermission::where('route_path', $path)
                    ->where('route_method', $method)
                    ->first();

                if (! $permission) {
                    RoutePermission::create([
                        'route_path' => $path,
                        'route_method' => $method,
                        'description' => 'Auto generated for '.($route->getName() ?? $path),
                        'allowed_roles' => [$defaultRole],
                        'is_active' => true,
                    ]);
                    $newCount++;
                }

                $totalCount++;
            }
        }

        if ($clean) {
            $existingPermissions = RoutePermission::all();
            foreach ($existingPermissions as $permission) {
                $exists = false;
                foreach ($processedRoutes as $processed) {
                    if ($processed['path'] === $permission->route_path
                        && $processed['method'] === $permission->route_method) {
                        $exists = true;
                        break;
                    }
                }

                if (! $exists) {
                    $permission->delete();
                    $removedCount++;
                }
            }
        }

        return [
            'scanned' => $totalCount,
            'created' => $newCount,
            'removed' => $removedCount,
            'prefix' => $prefix,
            'default_role' => $defaultRole,
        ];
    }
}