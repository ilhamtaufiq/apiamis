<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_blog_assets', function (Blueprint $table) {
            $table->foreignId('blog_id')->nullable()->after('user_id')->constrained('tbl_blog')->nullOnDelete();
        });
    }

    public function down(): void
    {
        Schema::table('tbl_blog_assets', function (Blueprint $table) {
            $table->dropConstrainedForeignId('blog_id');
        });
    }
};
