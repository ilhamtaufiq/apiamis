<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Crypt;

return new class extends Migration
{
    /**
     * Run the migrations.
     */
    public function up(): void
    {
        Schema::table('tbl_penerima', function (Blueprint $table) {
            $table->text('nik')->nullable()->change();
            $table->text('alamat')->nullable()->change();
        });

        // Encrypt existing data if it's not already encrypted
        $penerimas = DB::table('tbl_penerima')->get();
        foreach ($penerimas as $penerima) {
            $updates = [];
            
            // Check NIK
            if ($penerima->nik) {
                try {
                    Crypt::decryptString($penerima->nik);
                } catch (\Exception $e) {
                    $updates['nik'] = Crypt::encryptString($penerima->nik);
                }
            }
            
            // Check Alamat
            if ($penerima->alamat) {
                try {
                    Crypt::decryptString($penerima->alamat);
                } catch (\Exception $e) {
                    $updates['alamat'] = Crypt::encryptString($penerima->alamat);
                }
            }
            
            if (!empty($updates)) {
                DB::table('tbl_penerima')->where('id', $penerima->id)->update($updates);
            }
        }
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_penerima', function (Blueprint $table) {
            $table->string('nik', 255)->nullable()->change();
            $table->string('alamat', 255)->nullable()->change();
        });
    }
};
