<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    public function up(): void
    {
        // 1. Add JSON column to tbl_progress_items
        Schema::table('tbl_progress_items', function (Blueprint $table) {
            $table->json('weekly_data')->nullable()->after('target_volume');
        });

        // 2. Migrate existing data
        $items = DB::table('tbl_progress_items')->get();
        foreach ($items as $item) {
            $weeklyProgress = DB::table('tbl_weekly_progress')
                ->where('progress_item_id', $item->id)
                ->get();
            
            $weeklyData = [];
            foreach ($weeklyProgress as $wp) {
                $weeklyData[$wp->minggu] = [
                    'rencana' => (float) $wp->rencana,
                    'realisasi' => $wp->realisasi !== null ? (float) $wp->realisasi : null,
                ];
            }

            if (!empty($weeklyData)) {
                DB::table('tbl_progress_items')
                    ->where('id', $item->id)
                    ->update(['weekly_data' => json_encode($weeklyData)]);
            }
        }

        // 3. Drop tbl_weekly_progress table
        Schema::dropIfExists('tbl_weekly_progress');
    }

    public function down(): void
    {
        // 1. Recreate tbl_weekly_progress table
        Schema::create('tbl_weekly_progress', function (Blueprint $table) {
            $table->id();
            $table->foreignId('progress_item_id')->constrained('tbl_progress_items')->onDelete('cascade');
            $table->integer('minggu');
            $table->decimal('rencana', 15, 2)->default(0);
            $table->decimal('realisasi', 15, 2)->nullable();
            $table->timestamps();

            $table->unique(['progress_item_id', 'minggu']);
        });

        // 2. Restore data from JSON column
        $items = DB::table('tbl_progress_items')->whereNotNull('weekly_data')->get();
        foreach ($items as $item) {
            $weeklyData = json_decode($item->weekly_data, true);
            if (is_array($weeklyData)) {
                foreach ($weeklyData as $minggu => $data) {
                    DB::table('tbl_weekly_progress')->insert([
                        'progress_item_id' => $item->id,
                        'minggu' => $minggu,
                        'rencana' => $data['rencana'] ?? 0,
                        'realisasi' => $data['realisasi'] ?? null,
                        'created_at' => now(),
                        'updated_at' => now(),
                    ]);
                }
            }
        }

        // 3. Drop JSON column
        Schema::table('tbl_progress_items', function (Blueprint $table) {
            $table->dropColumn('weekly_data');
        });
    }
};
