<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * Share form kelembagaan SPAM: link publik + usulan update yang menunggu verifikasi admin.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::create('spam_kelembagaan_share_links', function (Blueprint $table) {
            $table->id();
            $table->foreignId('unit_spam_id')->constrained('tbl_unit_spam')->cascadeOnDelete();
            $table->foreignId('created_by')->nullable()->constrained('users')->nullOnDelete();
            $table->string('token', 64)->unique();
            $table->string('label')->nullable();
            $table->boolean('is_active')->default(true);
            $table->timestamp('expires_at')->nullable();
            $table->unsignedInteger('max_submissions')->nullable();
            $table->unsignedInteger('submission_count')->default(0);
            $table->text('admin_note')->nullable();
            $table->timestamps();

            $table->index(['unit_spam_id', 'is_active']);
        });

        Schema::create('spam_kelembagaan_submissions', function (Blueprint $table) {
            $table->id();
            $table->foreignId('share_link_id')
                ->constrained('spam_kelembagaan_share_links')
                ->cascadeOnDelete();
            $table->foreignId('unit_spam_id')->constrained('tbl_unit_spam')->cascadeOnDelete();
            $table->json('payload');
            $table->json('snapshot_before')->nullable();
            $table->string('submitter_name')->nullable();
            $table->string('submitter_phone', 50)->nullable();
            $table->string('submitter_instansi')->nullable();
            $table->text('submitter_note')->nullable();
            $table->string('status', 20)->default('pending'); // pending|approved|rejected
            $table->foreignId('reviewed_by')->nullable()->constrained('users')->nullOnDelete();
            $table->timestamp('reviewed_at')->nullable();
            $table->text('review_note')->nullable();
            $table->string('submitter_ip', 45)->nullable();
            $table->string('user_agent', 500)->nullable();
            $table->timestamps();

            $table->index(['status', 'created_at']);
            $table->index(['unit_spam_id', 'status']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('spam_kelembagaan_submissions');
        Schema::dropIfExists('spam_kelembagaan_share_links');
    }
};
