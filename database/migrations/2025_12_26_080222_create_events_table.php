<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * Run the migrations.
     */
    public function up(): void
    {
        Schema::create('tbl_events', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained('users')->onDelete('cascade');
            $table->string('title');
            $table->boolean('is_allday')->default(false);
            $table->dateTime('start');
            $table->dateTime('end');
            $table->string('category')->default('event'); // event, task, milestone, holiday
            $table->string('location')->nullable();
            $table->text('description')->nullable();
            $table->string('color')->nullable();
            $table->string('bg_color')->nullable();
            $table->string('border_color')->nullable();
            $table->timestamps();
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('tbl_events');
    }
};
