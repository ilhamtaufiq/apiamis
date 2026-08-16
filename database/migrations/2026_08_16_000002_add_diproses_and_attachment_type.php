<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        DB::statement("ALTER TABLE tbl_kontrak_addendums MODIFY status ENUM('draft','diajukan','diproses','disetujui','ditolak') NOT NULL DEFAULT 'draft'");

        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->string('attachment_type')->nullable()->after('addendum_id');
        });
    }

    public function down(): void
    {
        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->dropColumn('attachment_type');
        });

        DB::statement("ALTER TABLE tbl_kontrak_addendums MODIFY status ENUM('draft','diajukan','disetujui','ditolak') NOT NULL DEFAULT 'draft'");
    }
};
