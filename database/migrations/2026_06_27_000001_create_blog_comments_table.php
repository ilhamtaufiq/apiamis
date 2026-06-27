<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_blog_comment', function (Blueprint $table) {
            $table->id();
            $table->foreignId('blog_id')->constrained('tbl_blog')->cascadeOnDelete();
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->foreignId('parent_id')->nullable()->constrained('tbl_blog_comment')->nullOnDelete();
            $table->text('body');
            $table->unsignedTinyInteger('depth')->default(0);
            $table->softDeletes();
            $table->timestamps();

            $table->index(['blog_id', 'parent_id']);
            $table->index(['blog_id', 'created_at']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_blog_comment');
    }
};