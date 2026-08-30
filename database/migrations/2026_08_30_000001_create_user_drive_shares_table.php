<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('user_drive_shares', function (Blueprint $table) {
            $table->id();
            $table->foreignId('item_id')->constrained('user_drive_items')->cascadeOnDelete();
            // null = dibagikan ke semua user
            $table->foreignId('shared_to_user_id')->nullable()->constrained('users')->cascadeOnDelete();
            $table->timestamps();

            $table->unique(['item_id', 'shared_to_user_id']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('user_drive_shares');
    }
};
