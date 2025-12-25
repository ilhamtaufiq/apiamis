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
        Schema::create('tbl_tiket', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained('users')->onDelete('cascade');
            $table->foreignId('pekerjaan_id')->nullable()->constrained('tbl_pekerjaan')->onDelete('set null');
            $table->string('subjek');
            $table->text('deskripsi');
            $table->enum('kategori', ['bug', 'request', 'other'])->default('other');
            $table->enum('prioritas', ['low', 'medium', 'high'])->default('medium');
            $table->enum('status', ['open', 'pending', 'closed'])->default('open');
            $table->text('admin_notes')->nullable();
            $table->timestamps();
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('tbl_tiket');
    }
};
