<?php

namespace Database\Seeders;

use Illuminate\Database\Console\Seeds\WithoutModelEvents;
use Illuminate\Database\Seeder;
use Spatie\Permission\Models\Role;

class RoleSeeder extends Seeder
{
    /**
     * Run the database seeds.
     */
    public function run(): void
    {
        // Create default roles
        $roles = ['admin', 'tfl'];
        
        foreach ($roles as $roleName) {
            Role::firstOrCreate(['name' => $roleName]);
        }
        
        // Assign admin role to specific user
        $adminUser = \App\Models\User::where('email', 'ilhamtaufiq@gmail.com')->first();
        if ($adminUser) {
            $adminUser->syncRoles(['admin']);
            $this->command->info('Assigned admin role to ilhamtaufiq@gmail.com');
        } else {
            $this->command->warn('User ilhamtaufiq@gmail.com not found for role assignment');
        }

        $this->command->info('Default roles created: ' . implode(', ', $roles));
    }
}
