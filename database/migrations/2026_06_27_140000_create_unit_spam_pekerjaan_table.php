<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_unit_spam_pekerjaan', function (Blueprint $table) {
            $table->id();
            $table->foreignId('unit_spam_id')->constrained('tbl_unit_spam')->cascadeOnDelete();
            $table->foreignId('pekerjaan_id')->constrained('tbl_pekerjaan')->cascadeOnDelete();
            $table->foreignId('output_id')->nullable()->constrained('tbl_output')->nullOnDelete();
            $table->timestamps();

            $table->unique(['unit_spam_id', 'pekerjaan_id'], 'unit_spam_pekerjaan_unique');
            $table->index('pekerjaan_id');
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_unit_spam_pekerjaan');
    }
};