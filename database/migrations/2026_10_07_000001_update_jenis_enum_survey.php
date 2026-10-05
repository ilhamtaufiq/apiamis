<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    /**
     * Taksonomi jenis survey baru:
     * - spam_perpipaan, spam_pengeboran (dulu: spam, sumur_bor)
     * - mck_individu, mck_komunal (dulu: mck)
     */
    public function up(): void
    {
        DB::table('tbl_survey_lokasi')->where('jenis', 'spam')->update(['jenis' => 'spam_perpipaan']);
        DB::table('tbl_survey_lokasi')->where('jenis', 'sumur_bor')->update(['jenis' => 'spam_pengeboran']);
        DB::table('tbl_survey_lokasi')->where('jenis', 'mck')->update(['jenis' => 'mck_komunal']);

        DB::table('tbl_survey_tugas')->where('jenis', 'spam')->update(['jenis' => 'spam_perpipaan']);
        DB::table('tbl_survey_tugas')->where('jenis', 'sumur_bor')->update(['jenis' => 'spam_pengeboran']);
        DB::table('tbl_survey_tugas')->where('jenis', 'mck')->update(['jenis' => 'mck_komunal']);

        DB::statement("ALTER TABLE tbl_survey_lokasi MODIFY jenis ENUM('spam_perpipaan','spam_pengeboran','mck_individu','mck_komunal') NOT NULL");
        DB::statement("ALTER TABLE tbl_survey_tugas MODIFY jenis ENUM('spam_perpipaan','spam_pengeboran','mck_individu','mck_komunal') NULL");
    }

    public function down(): void
    {
        DB::statement("ALTER TABLE tbl_survey_lokasi MODIFY jenis ENUM('spam','sumur_bor','mck') NOT NULL");
        DB::statement("ALTER TABLE tbl_survey_tugas MODIFY jenis ENUM('spam','sumur_bor','mck') NULL");

        DB::table('tbl_survey_lokasi')->where('jenis', 'spam_perpipaan')->update(['jenis' => 'spam']);
        DB::table('tbl_survey_lokasi')->where('jenis', 'spam_pengeboran')->update(['jenis' => 'sumur_bor']);
        DB::table('tbl_survey_lokasi')->whereIn('jenis', ['mck_individu', 'mck_komunal'])->update(['jenis' => 'mck']);

        DB::table('tbl_survey_tugas')->where('jenis', 'spam_perpipaan')->update(['jenis' => 'spam']);
        DB::table('tbl_survey_tugas')->where('jenis', 'spam_pengeboran')->update(['jenis' => 'sumur_bor']);
        DB::table('tbl_survey_tugas')->whereIn('jenis', ['mck_individu', 'mck_komunal'])->update(['jenis' => 'mck']);
    }
};
