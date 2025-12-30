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
        Schema::create('simulation_networks', function (Blueprint $table) {
            $table->id();
            $table->string('name');
            $table->text('description')->nullable();

            // Owner relationship
            $table->foreignId('user_id')->constrained('users')->onDelete('cascade');

            // Optional link to pekerjaan (infrastructure project)
            $table->foreignId('pekerjaan_id')->nullable()->constrained('tbl_pekerjaan')->onDelete('set null');

            // Network data stored as JSON (junctions, reservoirs, tanks, pipes, pumps, valves)
            $table->json('network_data');

            // Simulation settings
            $table->json('simulation_settings')->nullable();

            // Last simulation results (optional, for caching)
            $table->json('last_results')->nullable();
            $table->timestamp('last_simulated_at')->nullable();

            // Version tracking for history
            $table->unsignedInteger('version')->default(1);

            // Sharing permissions
            $table->boolean('is_public')->default(false);

            // Soft delete for recovery
            $table->softDeletes();

            $table->timestamps();

            // Indexes for faster queries
            $table->index(['user_id', 'created_at']);
            $table->index('pekerjaan_id');
            $table->index('is_public');
        });

        // Version history table for undo/redo and audit trail
        Schema::create('simulation_network_versions', function (Blueprint $table) {
            $table->id();
            $table->foreignId('simulation_network_id')->constrained('simulation_networks')->onDelete('cascade');
            $table->unsignedInteger('version');
            $table->json('network_data');
            $table->json('simulation_settings')->nullable();
            $table->string('change_description')->nullable();
            $table->foreignId('changed_by')->constrained('users')->onDelete('cascade');
            $table->timestamp('created_at');

            // Composite index for version lookup
            $table->unique(['simulation_network_id', 'version']);
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('simulation_network_versions');
        Schema::dropIfExists('simulation_networks');
    }
};
