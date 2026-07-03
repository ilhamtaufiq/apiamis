<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('puspen_progress_fisik', function (Blueprint $table) {
            $table->boolean('pho_completed')->default(false)->after('realisasi');
        });
    }

    public function down(): void
    {
        Schema::table('puspen_progress_fisik', function (Blueprint $table) {
            $table->dropColumn('pho_completed');
        });
    }
};