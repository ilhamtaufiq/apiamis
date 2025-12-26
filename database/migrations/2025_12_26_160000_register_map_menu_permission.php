<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    /**
     * Run the migrations.
     */
    public function up(): void
    {
        // Get the Dokumentasi parent ID
        $parentId = DB::table('menu_permissions')
            ->where('menu_label', 'Dokumentasi')
            ->value('id');

        if ($parentId) {
            // Get the viewer permission ID
            $permissionId = DB::table('permissions')
                ->where('name', 'viewer')
                ->value('id');

            if ($permissionId) {
                DB::table('menu_permissions')->insert([
                    'menu_key' => 'map',
                    'menu_label' => 'Peta Progress',
                    'menu_path' => '/map',
                    'icon' => 'MapPin',
                    'parent_id' => $parentId,
                    'order' => 20,
                    'permission_id' => $permissionId,
                    'created_at' => now(),
                    'updated_at' => now(),
                ]);
            }
        }
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        DB::table('menu_permissions')->where('menu_key', 'map')->delete();
    }
};
