<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * Run the migrations.
     */
    public function up(): void
    {
        Schema::create('route_permissions', function (Blueprint $table) {
            $table->id();
            $table->string('route_path'); // e.g., '/pekerjaan/new'
            $table->string('route_method')->default('GET'); // GET, POST, PUT, DELETE, etc.
            $table->string('description')->nullable();
            $table->json('allowed_roles'); // Array of role names ['admin', 'operator']
            $table->boolean('is_active')->default(true);
            $table->timestamps();
            
            // Add index for faster queries
            $table->index(['route_path', 'route_method']);
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('route_permissions');
    }
};
