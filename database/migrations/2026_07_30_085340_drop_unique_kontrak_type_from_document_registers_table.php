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
        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->index('kontrak_id', 'tbl_document_registers_kontrak_id_index');
            $table->dropUnique('uq_document_registers_kontrak_type');
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->unique(['kontrak_id', 'type_id'], 'uq_document_registers_kontrak_type');
            $table->dropIndex('tbl_document_registers_kontrak_id_index');
        });
    }
};
