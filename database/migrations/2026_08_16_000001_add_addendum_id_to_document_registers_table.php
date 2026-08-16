<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->foreignId('addendum_id')
                ->nullable()
                ->after('type_id')
                ->constrained('tbl_kontrak_addendums')
                ->nullOnDelete();
        });
    }

    public function down(): void
    {
        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->dropConstrainedForeignId('addendum_id');
        });
    }
};
