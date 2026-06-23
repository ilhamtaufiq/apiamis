<?php

namespace Database\Seeders;

use Illuminate\Database\Console\Seeds\WithoutModelEvents;
use Illuminate\Database\Seeder;
use Illuminate\Support\Facades\DB;
use Spatie\Permission\Models\Role;
use App\Models\User;

class RoleSeeder extends Seeder
{
    /**
     * Run the database seeds.
     */
    public function run(): void
    {
        // Create default roles
        // 'pengawas' = user with role pengawas lapangan (used for Puspen KPI and assignments)
        $roles = ['admin', 'tfl', 'pengawas'];
        
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

        // Backfill: Give 'pengawas' role to any users who already have pekerjaan assigned
        // (via user_pekerjaan). This ensures existing "Pengawas Lapangan" appear in /puspen/pengawas-kpi.
        $assignedUserIds = DB::table('user_pekerjaan')->distinct()->pluck('user_id');
        if ($assignedUserIds->isNotEmpty()) {
            $pengawasUsers = User::whereIn('id', $assignedUserIds)->get();
            $count = 0;
            foreach ($pengawasUsers as $u) {
                if (!$u->hasRole('pengawas')) {
                    $u->assignRole('pengawas');
                    $count++;
                }
            }
            if ($count > 0) {
                $this->command->info("Assigned 'pengawas' role to {$count} user(s) who had pekerjaan assignments.");
            }
        }
    }
}
