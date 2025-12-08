<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class MenuPermission extends Model
{
    protected $fillable = [
        'menu_key',
        'menu_label',
        'menu_parent',
        'allowed_roles',
        'is_active',
    ];

    protected $casts = [
        'allowed_roles' => 'array',
        'is_active' => 'boolean',
    ];

    /**
     * Check if a user can access this menu based on their roles
     */
    public function canAccess($user): bool
    {
        if (!$this->is_active) {
            return true; // If menu permission is disabled, allow access
        }

        if (empty($this->allowed_roles)) {
            return true; // If no roles specified, allow all
        }

        $userRoles = $user->roles->pluck('name')->toArray();
        
        return !empty(array_intersect($userRoles, $this->allowed_roles));
    }

    /**
     * Get all menu permissions for a specific menu key
     */
    public static function findByMenuKey(string $menuKey)
    {
        return self::where('menu_key', $menuKey)
            ->where('is_active', true)
            ->first();
    }

    /**
     * Get all allowed menus for a user
     */
    public static function getAllowedMenus($user): array
    {
        $allMenus = self::where('is_active', true)->get();
        
        return $allMenus->filter(function ($menu) use ($user) {
            return $menu->canAccess($user);
        })->pluck('menu_key')->toArray();
    }
}
