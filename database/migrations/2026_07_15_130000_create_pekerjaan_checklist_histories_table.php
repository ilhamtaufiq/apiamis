<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('pekerjaan_checklist_histories', function (Blueprint $table) {
            $table->id();
            $table->foreignId('pekerjaan_id')->constrained('tbl_pekerjaan')->cascadeOnDelete();
            $table->foreignId('checklist_item_id')->constrained('tbl_checklist_items')->cascadeOnDelete();
            $table->boolean('is_checked');
            $table->text('notes')->nullable();
            $table->foreignId('user_id')->nullable()->constrained('users')->nullOnDelete();
            $table->timestamp('created_at')->useCurrent();

            $table->index(['pekerjaan_id', 'created_at']);
            $table->index(['checklist_item_id', 'created_at']);
            $table->index(['user_id', 'created_at']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('pekerjaan_checklist_histories');
    }
};
