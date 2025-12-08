<?php


namespace App\Http\Controllers;

use App\Models\MenuPermission;
use Illuminate\Http\Request;

class MenuPermissionController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(Request $request)
    {
        $query = MenuPermission::query();

        // Add search functionality
        if ($request->has('search') && $request->search) {
            $query->where(function($q) use ($request) {
                $q->where('menu_key', 'like', '%' . $request->search . '%')
                  ->orWhere('menu_label', 'like', '%' . $request->search . '%');
            });
        }

        // Filter by is_active
        if ($request->has('is_active')) {
            $query->where('is_active', $request->is_active);
        }

        return $query->orderBy('menu_label')->paginate(15);
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'menu_key' => 'required|string|unique:menu_permissions,menu_key',
            'menu_label' => 'required|string',
            'menu_parent' => 'nullable|string',
            'allowed_roles' => 'nullable|array',
            'is_active' => 'boolean',
        ]);

        $menuPermission = MenuPermission::create($validated);

        return response()->json($menuPermission, 201);
    }

    /**
     * Display the specified resource.
     */
    public function show(MenuPermission $menuPermission)
    {
        return $menuPermission;
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, MenuPermission $menuPermission)
    {
        $validated = $request->validate([
            'menu_key' => 'sometimes|string|unique:menu_permissions,menu_key,' . $menuPermission->id,
            'menu_label' => 'sometimes|string',
            'menu_parent' => 'nullable|string',
            'allowed_roles' => 'nullable|array',
            'is_active' => 'boolean',
        ]);

        $menuPermission->update($validated);

        return response()->json($menuPermission);
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(MenuPermission $menuPermission)
    {
        $menuPermission->delete();
        return response()->json(['message' => 'Menu permission deleted']);
    }

    /**
     * Get allowed menus for the current authenticated user
     */
    public function getUserMenus(Request $request)
    {
        $user = $request->user();
        
        if (!$user) {
            return response()->json([
                'allowed_menus' => [],
                'configured_menus' => []
            ], 401);
        }

        // Get all active menu permissions
        $allPermissions = MenuPermission::where('is_active', true)->get();
        
        // Get list of all menu keys that have permissions configured
        $configuredMenus = $allPermissions->pluck('menu_key')->toArray();

        // Get list of menu keys the user is allowed to access
        $allowedMenus = $allPermissions->filter(function ($menu) use ($user) {
            return $menu->canAccess($user);
        })->pluck('menu_key')->toArray();
        
        return response()->json([
            'allowed_menus' => array_values($allowedMenus),
            'configured_menus' => array_values($configuredMenus)
        ]);
    }
}
