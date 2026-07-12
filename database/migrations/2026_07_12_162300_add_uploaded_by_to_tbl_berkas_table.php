<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_berkas', function (Blueprint $table) {
            if (! Schema::hasColumn('tbl_berkas', 'uploaded_by')) {
                $table->foreignId('uploaded_by')
                    ->nullable()
                    ->after('jenis_dokumen')
                    ->constrained('users')
                    ->nullOnDelete();
            }
        });
    }

    public function down(): void
    {
        Schema::table('tbl_berkas', function (Blueprint $table) {
            if (Schema::hasColumn('tbl_berkas', 'uploaded_by')) {
                $table->dropConstrainedForeignId('uploaded_by');
            }
        });
    }
};
