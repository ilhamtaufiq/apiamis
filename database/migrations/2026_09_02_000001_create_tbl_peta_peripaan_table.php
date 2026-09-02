<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_peta_peripaan', function (Blueprint $table) {
            $table->id();
            $table->foreignId('pekerjaan_id')->nullable()->constrained('tbl_pekerjaan')->nullOnDelete();
            $table->string('nama');
            $table->json('geojson')->nullable();
            $table->foreignId('uploaded_by')->nullable()->constrained('users')->nullOnDelete();
            $table->timestamps();
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_peta_peripaan');
    }
};
