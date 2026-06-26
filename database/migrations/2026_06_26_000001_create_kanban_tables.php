<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_kanban_boards', function (Blueprint $table) {
            $table->id();
            $table->string('slug')->unique();
            $table->string('title');
            $table->text('description')->nullable();
            $table->timestamps();
        });

        Schema::create('tbl_kanban_columns', function (Blueprint $table) {
            $table->id();
            $table->foreignId('board_id')->constrained('tbl_kanban_boards')->cascadeOnDelete();
            $table->string('title');
            $table->unsignedInteger('position')->default(0);
            $table->enum('tiket_status', ['open', 'pending', 'closed'])->nullable();
            $table->string('color', 7)->nullable();
            $table->timestamps();
        });

        Schema::create('tbl_kanban_cards', function (Blueprint $table) {
            $table->id();
            $table->foreignId('board_id')->constrained('tbl_kanban_boards')->cascadeOnDelete();
            $table->foreignId('column_id')->constrained('tbl_kanban_columns')->cascadeOnDelete();
            $table->unsignedInteger('position')->default(0);
            $table->string('title');
            $table->text('description')->nullable();
            $table->string('status_label')->nullable();
            $table->foreignId('pekerjaan_id')->nullable()->constrained('tbl_pekerjaan')->nullOnDelete();
            $table->foreignId('tiket_id')->nullable()->constrained('tbl_tiket')->nullOnDelete();
            $table->enum('source', ['manual', 'tiket'])->default('manual');
            $table->json('metadata')->nullable();
            $table->foreignId('created_by')->constrained('users')->cascadeOnDelete();
            $table->timestamps();

            $table->unique(['board_id', 'tiket_id']);
        });

        $now = now();
        $boardId = DB::table('tbl_kanban_boards')->insertGetId([
            'slug' => 'organisasi',
            'title' => 'Kanban Organisasi',
            'description' => 'Papan kerja bersama organisasi ARUMANIS',
            'created_at' => $now,
            'updated_at' => $now,
        ]);

        $columns = [
            ['title' => 'Baru', 'position' => 0, 'tiket_status' => 'open', 'color' => '#3b82f6'],
            ['title' => 'Proses', 'position' => 1, 'tiket_status' => 'pending', 'color' => '#f59e0b'],
            ['title' => 'Selesai', 'position' => 2, 'tiket_status' => 'closed', 'color' => '#22c55e'],
        ];

        foreach ($columns as $column) {
            DB::table('tbl_kanban_columns')->insert([
                'board_id' => $boardId,
                'title' => $column['title'],
                'position' => $column['position'],
                'tiket_status' => $column['tiket_status'],
                'color' => $column['color'],
                'created_at' => $now,
                'updated_at' => $now,
            ]);
        }
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_kanban_cards');
        Schema::dropIfExists('tbl_kanban_columns');
        Schema::dropIfExists('tbl_kanban_boards');
    }
};