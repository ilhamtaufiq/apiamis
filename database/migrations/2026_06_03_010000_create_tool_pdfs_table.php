<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tool_pdfs', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained()->cascadeOnDelete();
            $table->foreignId('parent_id')->nullable()->constrained('tool_pdfs')->nullOnDelete();
            $table->string('name');
            $table->string('original_filename')->nullable();
            $table->string('kind', 20)->default('source');
            $table->timestamps();
            $table->softDeletes();

            $table->index(['user_id', 'kind']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tool_pdfs');
    }
};
