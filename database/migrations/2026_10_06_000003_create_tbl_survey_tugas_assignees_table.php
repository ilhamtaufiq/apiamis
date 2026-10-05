<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_survey_tugas_assignees', function (Blueprint $table) {
            $table->id();
            $table->foreignId('survey_tugas_id')->constrained('tbl_survey_tugas')->cascadeOnDelete();
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->timestamps();
            $table->unique(['survey_tugas_id', 'user_id'], 'tugas_assignee_unique');
        });

        // Backfill dari assignee_id yang sudah ada
        $rows = DB::table('tbl_survey_tugas')->whereNotNull('assignee_id')->get(['id', 'assignee_id']);
        foreach ($rows as $row) {
            DB::table('tbl_survey_tugas_assignees')->updateOrInsert(
                ['survey_tugas_id' => $row->id, 'user_id' => $row->assignee_id],
                ['created_at' => now(), 'updated_at' => now()]
            );
        }
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_survey_tugas_assignees');
    }
};
