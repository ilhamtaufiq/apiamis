<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tool_pdf_signature_placements', function (Blueprint $table) {
            $table->id();
            $table->foreignId('tool_pdf_id')->constrained('tool_pdfs')->cascadeOnDelete();
            $table->uuid('signature_id');
            $table->unsignedInteger('page_number');
            $table->decimal('x_ratio', 8, 6);
            $table->decimal('y_ratio', 8, 6);
            $table->decimal('scale', 8, 6);
            $table->unsignedInteger('sort_order')->default(0);
            $table->string('signature_name', 255);
            $table->string('signature_file_name', 255);
            $table->string('signature_mime_type', 100);
            $table->unsignedInteger('signature_width');
            $table->unsignedInteger('signature_height');
            $table->longText('signature_data_url')->nullable();
            $table->enum('signature_source_type', ['upload', 'library'])->nullable();
            $table->string('signature_source_id', 64)->nullable();
            $table->timestamps();

            $table->index(['tool_pdf_id', 'page_number']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tool_pdf_signature_placements');
    }
};
