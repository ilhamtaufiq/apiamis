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
        Schema::create('tbl_spam_budgets', function (Blueprint $table) {
            $table->id();
            $table->unsignedBigInteger('unit_spam_id');
            $table->double('nilai_kontrak');
            $table->string('tahun', 4);
            $table->string('nama_paket', 255);
            $table->string('sumber_dana', 50)->default('APBD');
            $table->timestamps();

            $table->foreign('unit_spam_id')
                ->references('id')
                ->on('tbl_unit_spam')
                ->onDelete('cascade');
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('tbl_spam_budgets');
    }
};
