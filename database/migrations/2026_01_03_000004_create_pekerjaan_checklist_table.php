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
        Schema::create('pekerjaan_checklist', function (Blueprint $table) {
            $table->id();
            $table->foreignId('pekerjaan_id')->constrained('tbl_pekerjaan')->onDelete('cascade');
            $table->foreignId('checklist_item_id')->constrained('tbl_checklist_items')->onDelete('cascade');
            $table->boolean('is_checked')->default(false);
            $table->timestamp('checked_at')->nullable();
            $table->foreignId('checked_by')->nullable()->constrained('users')->nullOnDelete();
            $table->text('notes')->nullable();
            $table->timestamps();

            // Unique constraint
            $table->unique(['pekerjaan_id', 'checklist_item_id']);
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('pekerjaan_checklist');
    }
};
