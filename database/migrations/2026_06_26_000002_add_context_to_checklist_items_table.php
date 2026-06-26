<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_checklist_items', function (Blueprint $table) {
            $table->string('context', 30)->default('pekerjaan')->after('sort_order');
            $table->index('context');
        });
    }

    public function down(): void
    {
        Schema::table('tbl_checklist_items', function (Blueprint $table) {
            $table->dropIndex(['context']);
            $table->dropColumn('context');
        });
    }
};