<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_live_chat_thread', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->string('status', 20)->default('open');
            $table->timestamp('last_message_at')->nullable();
            $table->timestamps();

            $table->unique('user_id');
            $table->index(['status', 'last_message_at']);
        });

        Schema::create('tbl_live_chat_message', function (Blueprint $table) {
            $table->id();
            $table->foreignId('thread_id')->constrained('tbl_live_chat_thread')->cascadeOnDelete();
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->text('message');
            $table->timestamp('read_at')->nullable();
            $table->timestamps();

            $table->index(['thread_id', 'id']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_live_chat_message');
        Schema::dropIfExists('tbl_live_chat_thread');
    }
};