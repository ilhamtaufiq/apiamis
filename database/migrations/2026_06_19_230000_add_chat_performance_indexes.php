<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('chat_sessions', function (Blueprint $table) {
            $table->index(['user_id', 'updated_at'], 'idx_chat_sessions_user_updated');
        });

        Schema::table('chat_messages', function (Blueprint $table) {
            $table->index(['chat_session_id', 'role', 'id'], 'idx_chat_messages_session_role_id');
        });
    }

    public function down(): void
    {
        Schema::table('chat_messages', function (Blueprint $table) {
            $table->dropIndex('idx_chat_messages_session_role_id');
        });
        Schema::table('chat_sessions', function (Blueprint $table) {
            $table->dropIndex('idx_chat_sessions_user_updated');
        });
    }
};
