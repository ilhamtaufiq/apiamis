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
        Schema::table('tbl_pekerjaan', function (Blueprint $table) {
            $table->foreignId('pengawas_id')->nullable()->constrained('pengawas')->nullOnDelete();
            $table->foreignId('pendamping_id')->nullable()->constrained('pengawas')->nullOnDelete();
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_pekerjaan', function (Blueprint $table) {
            $table->dropForeign(['pengawas_id']);
            $table->dropForeign(['pendamping_id']);
            $table->dropColumn(['pengawas_id', 'pendamping_id']);
        });
    }
};
