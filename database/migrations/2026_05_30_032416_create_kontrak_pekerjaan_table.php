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
        Schema::create('kontrak_pekerjaan', function (Blueprint $table) {
            $table->id();
            $table->unsignedBigInteger('kontrak_id');
            $table->unsignedBigInteger('pekerjaan_id');
            $table->timestamps();

            $table->foreign('kontrak_id')->references('id')->on('tbl_kontrak')->onDelete('cascade');
            $table->foreign('pekerjaan_id')->references('id')->on('tbl_pekerjaan')->onDelete('cascade');
            
            $table->unique(['kontrak_id', 'pekerjaan_id']);
        });

        // Migrate existing data
        \DB::statement('INSERT INTO kontrak_pekerjaan (kontrak_id, pekerjaan_id, created_at, updated_at) 
                        SELECT id, id_pekerjaan, NOW(), NOW() 
                        FROM tbl_kontrak 
                        WHERE id_pekerjaan IS NOT NULL');

        // Optional: We can drop id_pekerjaan from tbl_kontrak later, for now we keep it to prevent immediate breaking.
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('kontrak_pekerjaan');
    }
};
