<?php

namespace App\Console\Commands;

use Illuminate\Console\Command;
use Illuminate\Support\Facades\Route;
use App\Models\RoutePermission;

class SyncRoutePermissions extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'app:sync-route-permissions 
                            {--prefix=api : The route prefix to scan} 
                            {--role=admin : The default role to assign to new routes}
                            {--clean : Whether to remove permissions for routes that no longer exist}';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Automatically generate route permissions from registered Laravel routes';

    /**
     * Execute the console command.
     */
    public function handle()
    {
        $prefix = $this->option('prefix');
        $defaultRole = $this->option('role');
        $clean = $this->option('clean');
        
        $routes = Route::getRoutes();
        $processedRoutes = [];
        $newCount = 0;
        $totalCount = 0;

        $this->info("Scanning routes with prefix: {$prefix}...");

        foreach ($routes as $route) {
            $uri = $route->uri();
            
            // Skip routes that don't match the prefix
            if ($prefix && !str_starts_with($uri, $prefix)) {
                continue;
            }

            // Clean URI: remove 'api/' if present
            $cleanUri = $uri;
            if (str_starts_with($uri, 'api/')) {
                $cleanUri = substr($uri, 4);
            }

            // Convert Laravel {param} or {param?} to :param
            $path = '/' . preg_replace('/\{([a-zA-Z0-9_?]+)\}/', ':$1', $cleanUri);
            $path = str_replace('?', '', $path); // Remove optional parameter markers
            $path = preg_replace('/\/+/', '/', $path); // Clean double slashes
            $path = rtrim($path, '/');
            if (empty($path)) $path = '/';

            $methods = array_filter($route->methods(), fn($m) => $m !== 'HEAD');

            foreach ($methods as $method) {
                $processedRoutes[] = [
                    'path' => $path,
                    'method' => $method
                ];

                $permission = RoutePermission::where('route_path', $path)
                    ->where('route_method', $method)
                    ->first();

                if (!$permission) {
                    RoutePermission::create([
                        'route_path' => $path,
                        'route_method' => $method,
                        'description' => 'Auto generated for ' . ($route->getName() ?? $path),
                        'allowed_roles' => [$defaultRole],
                        'is_active' => true,
                    ]);
                    $this->line("  [NEW] {$method} {$path}");
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
                    if ($processed['path'] === $permission->route_path && 
                        $processed['method'] === $permission->route_method) {
                        $exists = true;
                        break;
                    }
                }

                if (!$exists) {
                    $this->warn("  [REMOVING] {$permission->route_method} {$permission->route_path} (Route no longer exists)");
                    $permission->delete();
                }
            }
        }

        $this->info("Done! Scanned {$totalCount} routes. Created {$newCount} new entries.");
        
        return self::SUCCESS;
    }
}
