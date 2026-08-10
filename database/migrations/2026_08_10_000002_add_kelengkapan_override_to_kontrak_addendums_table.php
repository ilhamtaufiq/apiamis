<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_kontrak_addendums', function (Blueprint $table) {
            $table->boolean('kelengkapan_override')->default(false)->after('status');
        });
    }

    public function down(): void
    {
        Schema::table('tbl_kontrak_addendums', function (Blueprint $table) {
            $table->dropColumn('kelengkapan_override');
        });
    }
};
