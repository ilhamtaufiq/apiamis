<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('user_drive_items', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained()->cascadeOnDelete();
            $table->foreignId('parent_id')->nullable()->constrained('user_drive_items')->nullOnDelete();
            $table->string('name');
            $table->string('kind', 20);
            $table->string('original_filename')->nullable();
            $table->timestamps();
            $table->softDeletes();

            $table->index(['user_id', 'parent_id', 'kind']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('user_drive_items');
    }
};