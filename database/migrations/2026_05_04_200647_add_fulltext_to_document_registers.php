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
            $table->fullText(['nomor', 'description'], 'ft_document_registers_search');
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->dropFullText('ft_document_registers_search');
        });
    }
};
