<?php

namespace App\Console\Commands;

use App\Services\RoutePermissionSyncService;
use Illuminate\Console\Command;

class SyncRoutePermissions extends Command
{
    /**
     * @var string
     */
    protected $signature = 'app:sync-route-permissions 
                            {--prefix=api : The route prefix to scan} 
                            {--role=admin : The default role to assign to new routes}
                            {--clean : Whether to remove permissions for routes that no longer exist}';

    /**
     * @var string
     */
    protected $description = 'Automatically generate route permissions from registered Laravel routes';

    public function handle(RoutePermissionSyncService $syncService): int
    {
        $prefix = (string) $this->option('prefix');
        $defaultRole = (string) $this->option('role');
        $clean = (bool) $this->option('clean');

        $this->info("Scanning routes with prefix: {$prefix}...");

        $result = $syncService->sync($prefix, $defaultRole, $clean);

        if ($clean && $result['removed'] > 0) {
            $this->warn("Removed {$result['removed']} stale route permission(s).");
        }

        $this->info("Done! Scanned {$result['scanned']} routes. Created {$result['created']} new entries.");

        return self::SUCCESS;
    }
}