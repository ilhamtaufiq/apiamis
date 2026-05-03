<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        // Add type column to support different sequences for different documents
        Schema::table('tbl_document_sequences', function (Blueprint $table) {
            // Remove unique on year since it will now be unique on (year, type)
            $table->dropUnique(['year']);
            $table->string('type')->default('global')->after('year');
            $table->unique(['year', 'type']);
        });

        // Create a table to track every generated document number
        Schema::create('tbl_document_logs', function (Blueprint $table) {
            $table->id();
            $table->string('type'); // sppbj, spk, spmk, etc
            $table->integer('year');
            $table->integer('sequence_number');
            $table->string('full_number');
            $table->foreignId('id_pekerjaan')->nullable()->constrained('tbl_pekerjaan');
            $table->foreignId('id_user')->nullable()->constrained('users');
            $table->enum('status', ['active', 'canceled'])->default('active');
            $table->text('cancel_reason')->nullable();
            $table->timestamps();
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_document_logs');
        
        Schema::table('tbl_document_sequences', function (Blueprint $table) {
            $table->dropUnique(['year', 'type']);
            $table->dropColumn('type');
            $table->unique(['year']);
        });
    }
};
