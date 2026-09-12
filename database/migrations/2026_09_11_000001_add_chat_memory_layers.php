<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('chat_sessions', function (Blueprint $table) {
            $table->text('context_summary')->nullable();
            $table->unsignedBigInteger('summary_upto_id')->nullable();
        });

        Schema::create('chat_user_memories', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained('users')->onDelete('cascade');
            $table->string('fact_hash', 64);
            $table->text('fact');
            $table->float('score')->default(1.0);
            $table->unsignedInteger('hit_count')->default(0);
            $table->timestamps();

            $table->unique(['user_id', 'fact_hash']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('chat_user_memories');

        Schema::table('chat_sessions', function (Blueprint $table) {
            $table->dropColumn(['context_summary', 'summary_upto_id']);
        });
    }
};
