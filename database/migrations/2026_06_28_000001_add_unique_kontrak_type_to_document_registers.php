<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        DB::statement('
            DELETE t1 FROM tbl_document_registers t1
            INNER JOIN tbl_document_registers t2
                ON t1.kontrak_id = t2.kontrak_id
                AND t1.type_id = t2.type_id
                AND t1.id < t2.id
        ');

        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->unique(['kontrak_id', 'type_id'], 'uq_document_registers_kontrak_type');
        });
    }

    public function down(): void
    {
        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->dropUnique('uq_document_registers_kontrak_type');
        });
    }
};