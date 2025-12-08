<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Casts\Attribute;

class RoutePermission extends Model
{
    protected $fillable = [
        'route_path',
        'route_method',
        'description',
        'allowed_roles',
        'is_active',
    ];

    protected $casts = [
        'allowed_roles' => 'array',
        'is_active' => 'boolean',
    ];

    /**
     * Check if a user can access this route based on their roles
     */
    public function canAccess($user): bool
    {
        if (!$this->is_active) {
            return true; // If route permission is disabled, allow access
        }

        if (empty($this->allowed_roles)) {
            return true; // If no roles specified, allow all
        }

        $userRoles = $user->roles->pluck('name')->toArray();
        
        return !empty(array_intersect($userRoles, $this->allowed_roles));
    }

    /**
     * Get all route permissions for a specific route path and method
     * Supports pattern matching for dynamic routes like /pekerjaan/:id
     */
    public static function findByRoute(string $path, string $method = 'GET')
    {
        $method = strtoupper($method);
        
        // First, try exact match
        $exactMatch = self::where('route_path', $path)
            ->where('route_method', $method)
            ->where('is_active', true)
            ->first();
            
        if ($exactMatch) {
            return $exactMatch;
        }
        
        // If no exact match, try pattern matching with :param placeholders
        $allRoutes = self::where('route_method', $method)
            ->where('is_active', true)
            ->get();
            
        foreach ($allRoutes as $route) {
            if (self::matchesPattern($route->route_path, $path)) {
                return $route;
            }
        }
        
        return null;
    }
    
    /**
     * Check if a route pattern matches an actual path
     * Pattern: /pekerjaan/:id matches /pekerjaan/397
     * Pattern: /users/:userId/edit matches /users/5/edit
     */
    public static function matchesPattern(string $pattern, string $path): bool
    {
        // If pattern doesn't contain :, it's not a dynamic route
        if (!str_contains($pattern, ':')) {
            return $pattern === $path;
        }
        
        // Convert pattern to regex
        // :id, :userId, :pekerjaanId etc. -> matches any number or alphanumeric
        $regex = preg_replace('/:[a-zA-Z_]+/', '([^/]+)', $pattern);
        $regex = '#^' . $regex . '$#';
        
        return (bool) preg_match($regex, $path);
    }
}
