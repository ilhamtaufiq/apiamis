<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_spse_sessions', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->text('encrypted_cookies');
            $table->string('lpse_slug', 64)->default('cianjurkab');
            $table->timestamp('expires_at')->nullable();
            $table->timestamp('last_validated_at')->nullable();
            $table->boolean('is_active')->default(true);
            $table->timestamps();

            $table->index(['user_id', 'is_active']);
        });

        Schema::create('tbl_procurement_sync_runs', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->nullable()->constrained('users')->nullOnDelete();
            $table->string('status', 32)->default('running');
            $table->unsignedInteger('item_count')->default(0);
            $table->unsignedInteger('matched_count')->default(0);
            $table->text('error_log')->nullable();
            $table->timestamp('started_at')->useCurrent();
            $table->timestamp('finished_at')->nullable();
        });

        Schema::create('tbl_procurement_staging_paket', function (Blueprint $table) {
            $table->id();
            $table->foreignId('sync_run_id')->constrained('tbl_procurement_sync_runs')->cascadeOnDelete();
            $table->string('sumber', 32);
            $table->string('kode_paket', 32)->index();
            $table->string('nama_paket', 500);
            $table->string('status_paket', 128)->nullable();
            $table->string('metode_pengadaan', 128)->nullable();
            $table->string('jenis_paket', 64)->nullable();
            $table->foreignId('matched_pekerjaan_id')->nullable()->constrained('tbl_pekerjaan')->nullOnDelete();
            $table->foreignId('matched_kontrak_id')->nullable()->constrained('tbl_kontrak')->nullOnDelete();
            $table->string('match_status', 32)->default('unmatched');
            $table->json('raw_row')->nullable();
            $table->timestamp('fetched_at')->useCurrent();
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_procurement_staging_paket');
        Schema::dropIfExists('tbl_procurement_sync_runs');
        Schema::dropIfExists('tbl_spse_sessions');
    }
};