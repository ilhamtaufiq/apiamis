<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::dropIfExists('tbl_rka_items');
        Schema::dropIfExists('tbl_rka_documents');
    }

    public function down(): void
    {
        // Fitur RKA telah dihapus; tidak ada rollback.
    }
};