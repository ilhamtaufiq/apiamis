<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('puspen_media_shares', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained()->cascadeOnDelete();
            $table->string('title');
            $table->text('description')->nullable();
            $table->string('share_token', 64)->unique();
            $table->boolean('is_public')->default(true);
            $table->timestamp('expires_at')->nullable();
            $table->unsignedInteger('download_count')->default(0);
            $table->timestamp('last_downloaded_at')->nullable();
            $table->timestamps();
            $table->softDeletes();

            $table->index(['user_id', 'is_public']);
            $table->index('expires_at');
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('puspen_media_shares');
    }
};
