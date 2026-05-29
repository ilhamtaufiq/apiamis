<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_kontrak_addendums', function (Blueprint $table) {
            $table->id();
            $table->foreignId('kontrak_id')->constrained('tbl_kontrak')->cascadeOnDelete();
            $table->unsignedInteger('addendum_ke');
            $table->string('nomor_addendum')->nullable();
            $table->date('tanggal_addendum');
            $table->enum('jenis_addendum', ['teknis', 'biaya', 'waktu', 'teknis_biaya', 'lainnya'])->default('lainnya');
            $table->text('alasan')->nullable();
            $table->text('deskripsi_perubahan')->nullable();
            $table->decimal('nilai_kontrak_sebelum', 18, 2)->nullable();
            $table->decimal('nilai_kontrak_sesudah', 18, 2)->nullable();
            $table->date('tgl_selesai_sebelum')->nullable();
            $table->date('tgl_selesai_sesudah')->nullable();
            $table->enum('status', ['draft', 'diajukan', 'disetujui', 'ditolak'])->default('draft');
            $table->foreignId('created_by')->nullable()->constrained('users')->nullOnDelete();
            $table->foreignId('approved_by')->nullable()->constrained('users')->nullOnDelete();
            $table->timestamp('approved_at')->nullable();
            $table->timestamps();

            $table->unique(['kontrak_id', 'addendum_ke']);
            $table->index(['status', 'tanggal_addendum']);
        });

        Schema::create('tbl_kontrak_addendum_items', function (Blueprint $table) {
            $table->id();
            $table->foreignId('addendum_id')->constrained('tbl_kontrak_addendums')->cascadeOnDelete();
            $table->string('nama_item')->nullable();
            $table->text('spesifikasi_sebelum')->nullable();
            $table->text('spesifikasi_sesudah')->nullable();
            $table->decimal('volume_sebelum', 18, 4)->nullable();
            $table->decimal('volume_sesudah', 18, 4)->nullable();
            $table->decimal('harga_sebelum', 18, 2)->nullable();
            $table->decimal('harga_sesudah', 18, 2)->nullable();
            $table->decimal('subtotal_sebelum', 18, 2)->nullable();
            $table->decimal('subtotal_sesudah', 18, 2)->nullable();
            $table->timestamps();
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_kontrak_addendum_items');
        Schema::dropIfExists('tbl_kontrak_addendums');
    }
};
