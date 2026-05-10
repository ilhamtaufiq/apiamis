<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    /**
     * Run the migrations.
     */
    public function up(): void
    {
        // Using raw SQL for MySQL enum update
        DB::statement("ALTER TABLE tbl_tiket MODIFY COLUMN kategori ENUM('bug', 'request', 'lapangan', 'other') DEFAULT 'other'");
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        DB::statement("ALTER TABLE tbl_tiket MODIFY COLUMN kategori ENUM('bug', 'request', 'other') DEFAULT 'other'");
    }
};
