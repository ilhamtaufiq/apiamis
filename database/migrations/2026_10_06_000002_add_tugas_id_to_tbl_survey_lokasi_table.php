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
        Schema::table('tbl_survey_lokasi', function (Blueprint $table) {
            $table->foreignId('tugas_id')->nullable()->after('user_id')->constrained('tbl_survey_tugas')->nullOnDelete();
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_survey_lokasi', function (Blueprint $table) {
            $table->dropConstrainedForeignId('tugas_id');
        });
    }
};
