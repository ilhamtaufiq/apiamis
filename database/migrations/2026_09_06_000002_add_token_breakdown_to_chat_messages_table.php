<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('chat_messages', function (Blueprint $table) {
            $table->unsignedInteger('prompt_tokens')->nullable()->after('tokens_used');
            $table->unsignedInteger('completion_tokens')->nullable()->after('prompt_tokens');
        });
    }

    public function down(): void
    {
        Schema::table('chat_messages', function (Blueprint $table) {
            $table->dropColumn(['prompt_tokens', 'completion_tokens']);
        });
    }
};
