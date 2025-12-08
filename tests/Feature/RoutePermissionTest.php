<?php

namespace Tests\Feature;

use App\Models\RoutePermission;
use App\Models\User;
use Illuminate\Foundation\Testing\RefreshDatabase;
use Illuminate\Foundation\Testing\WithFaker;
use Spatie\Permission\Models\Role;
use Tests\TestCase;

class RoutePermissionTest extends TestCase
{
    use RefreshDatabase;

    protected function setUp(): void
    {
        parent::setUp();
        
        // Create roles
        Role::create(['name' => 'admin']);
        Role::create(['name' => 'user']);
    }

    public function test_route_without_permission_is_denied_for_non_admin()
    {
        $user = User::factory()->create();
        $user->assignRole('user');

        $response = $this->actingAs($user)->getJson('/api/pekerjaan');

        // Route without permission config should be denied (deny by default)
        $response->assertStatus(403);
    }

    public function test_admin_can_access_any_route_without_permission()
    {
        $admin = User::factory()->create();
        $admin->assignRole('admin');

        $response = $this->actingAs($admin)->getJson('/api/pekerjaan');

        // Admin should bypass all permission checks
        $this->assertNotEquals(403, $response->status());
    }

    public function test_route_with_permission_is_restricted()
    {
        // Create a restricted route permission
        RoutePermission::create([
            'route_path' => '/pekerjaan',
            'route_method' => 'GET',
            'allowed_roles' => ['admin'],
            'is_active' => true,
        ]);

        $user = User::factory()->create();
        $user->assignRole('user');

        $response = $this->actingAs($user)->getJson('/api/pekerjaan');

        $response->assertStatus(403);
    }

    public function test_route_with_permission_is_accessible_by_allowed_role()
    {
        // Create a restricted route permission
        RoutePermission::create([
            'route_path' => '/pekerjaan',
            'route_method' => 'GET',
            'allowed_roles' => ['admin'],
            'is_active' => true,
        ]);

        $admin = User::factory()->create();
        $admin->assignRole('admin');

        $response = $this->actingAs($admin)->getJson('/api/pekerjaan');

        $this->assertNotEquals(403, $response->status());
    }

    public function test_route_permission_path_normalization()
    {
        // Test that /api/pekerjaan matches /pekerjaan
        RoutePermission::create([
            'route_path' => '/pekerjaan',
            'route_method' => 'GET',
            'allowed_roles' => ['admin'],
            'is_active' => true,
        ]);

        $user = User::factory()->create();
        $user->assignRole('user');

        // Request to /api/pekerjaan should match /pekerjaan permission
        $response = $this->actingAs($user)->getJson('/api/pekerjaan');

        $response->assertStatus(403);
    }
}
