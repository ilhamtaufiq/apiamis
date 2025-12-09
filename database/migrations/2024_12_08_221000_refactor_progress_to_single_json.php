<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    public function up(): void
    {
        // 1. Create tbl_progress
        Schema::create('tbl_progress', function (Blueprint $table) {
            $table->id();
            $table->foreignId('pekerjaan_id')->constrained('tbl_pekerjaan')->onDelete('cascade');
            $table->json('content')->nullable();
            $table->timestamps();
        });

        // 2. Migrate existing data
        // Group items by pekerjaan_id
        $items = DB::table('tbl_progress_items')->orderBy('id')->get();
        $groupedItems = $items->groupBy('pekerjaan_id');

        foreach ($groupedItems as $pekerjaanId => $projectItems) {
            $jsonItems = [];
            foreach ($projectItems as $item) {
                $weeklyData = json_decode($item->weekly_data ?? '[]', true);
                
                $jsonItems[] = [
                    'id' => $item->id, // Keep ID for reference if needed, though it won't be a DB ID anymore
                    'nama_item' => $item->nama_item,
                    'rincian_item' => $item->rincian_item,
                    'satuan' => $item->satuan,
                    'harga_satuan' => (float) $item->harga_satuan,
                    'bobot' => (float) $item->bobot,
                    'target_volume' => (float) $item->target_volume,
                    'weekly_data' => $weeklyData,
                ];
            }

            if (!empty($jsonItems)) {
                DB::table('tbl_progress')->insert([
                    'pekerjaan_id' => $pekerjaanId,
                    'content' => json_encode(['items' => $jsonItems]),
                    'created_at' => now(),
                    'updated_at' => now(),
                ]);
            }
        }

        // 3. Drop tbl_progress_items
        Schema::dropIfExists('tbl_progress_items');
    }

    public function down(): void
    {
        // 1. Recreate tbl_progress_items
        Schema::create('tbl_progress_items', function (Blueprint $table) {
            $table->id();
            $table->foreignId('pekerjaan_id')->constrained('tbl_pekerjaan')->onDelete('cascade');
            $table->string('nama_item');
            $table->string('rincian_item')->nullable();
            $table->string('satuan');
            $table->decimal('harga_satuan', 15, 2)->default(0);
            $table->decimal('bobot', 5, 2)->default(0);
            $table->decimal('target_volume', 15, 2)->default(0);
            $table->json('weekly_data')->nullable();
            $table->timestamps();
        });

        // 2. Restore data
        $progresses = DB::table('tbl_progress')->get();
        foreach ($progresses as $progress) {
            $content = json_decode($progress->content, true);
            $items = $content['items'] ?? [];
            
            foreach ($items as $item) {
                DB::table('tbl_progress_items')->insert([
                    'pekerjaan_id' => $progress->pekerjaan_id,
                    'nama_item' => $item['nama_item'],
                    'rincian_item' => $item['rincian_item'] ?? null,
                    'satuan' => $item['satuan'],
                    'harga_satuan' => $item['harga_satuan'],
                    'bobot' => $item['bobot'],
                    'target_volume' => $item['target_volume'],
                    'weekly_data' => json_encode($item['weekly_data'] ?? []),
                    'created_at' => now(),
                    'updated_at' => now(),
                ]);
            }
        }

        // 3. Drop tbl_progress
        Schema::dropIfExists('tbl_progress');
    }
};
