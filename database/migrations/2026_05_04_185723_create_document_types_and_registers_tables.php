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
        Schema::create('tbl_document_types', function (Blueprint $table) {
            $table->id();
            $table->string('name'); // e.g. "Berita Acara", "NPHD"
            $table->string('code')->unique(); // e.g. "BA", "NPHD"
            $table->string('format_template')->nullable(); // e.g. "{sequence}/BA-AMIS/{month}/{year}"
            $table->timestamps();
        });

        Schema::create('tbl_document_registers', function (Blueprint $table) {
            $table->id();
            $table->foreignId('kontrak_id')->constrained('tbl_kontrak')->onDelete('cascade');
            $table->foreignId('type_id')->constrained('tbl_document_types')->onDelete('cascade');
            $table->string('nomor')->unique();
            $table->date('tanggal');
            $table->integer('sequence_number');
            $table->integer('year');
            $table->text('description')->nullable();
            $table->timestamps();
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_document_registers');
        Schema::dropIfExists('tbl_document_types');
    }
};
