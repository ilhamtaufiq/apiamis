<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('chat_sessions', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained('users')->onDelete('cascade');
            $table->string('title', 255)->default('Percakapan Baru');
            $table->timestamps();
        });

        Schema::create('chat_messages', function (Blueprint $table) {
            $table->id();
            $table->foreignId('chat_session_id')->constrained('chat_sessions')->onDelete('cascade');
            $table->enum('role', ['user', 'assistant']);
            $table->longText('content');
            $table->json('tool_calls')->nullable();
            $table->unsignedInteger('tokens_used')->nullable();
            $table->timestamps();
        });

        // Cache table for AI knowledge (learned Q&A pairs)
        Schema::create('chat_knowledge_cache', function (Blueprint $table) {
            $table->id();
            $table->string('query_hash', 64)->unique(); // SHA-256 hash of normalized query
            $table->text('query');                       // Original user query
            $table->text('context_summary');             // Compressed context used
            $table->longText('response');                // AI response
            $table->unsignedInteger('hit_count')->default(0);
            $table->float('quality_score')->unsigned()->default(0.5); // 0-1, can be adjusted
            $table->timestamps();

            $table->index('query_hash');
            $table->index('hit_count');
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('chat_knowledge_cache');
        Schema::dropIfExists('chat_messages');
        Schema::dropIfExists('chat_sessions');
    }
};
