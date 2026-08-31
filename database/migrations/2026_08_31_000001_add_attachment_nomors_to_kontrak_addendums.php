<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_kontrak_addendums', function (Blueprint $table) {
            $table->json('attachment_nomors')->nullable()->after('nomor_addendum');
        });
    }

    public function down(): void
    {
        Schema::table('tbl_kontrak_addendums', function (Blueprint $table) {
            $table->dropColumn('attachment_nomors');
        });
    }
};
