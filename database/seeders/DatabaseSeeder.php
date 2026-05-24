<?php

namespace Database\Seeders;

use App\Models\User;
use Illuminate\Database\Console\Seeds\WithoutModelEvents;
use Illuminate\Database\Seeder;

class DatabaseSeeder extends Seeder
{
    use WithoutModelEvents;

    /**
     * Seed the application's database.
     */
    public function run(): void
    {
        // Disable FK checks so SPAM seeders can insert
        // without requiring tbl_desa to be pre-populated
        \Illuminate\Support\Facades\Schema::disableForeignKeyConstraints();

        $this->call(TblUnitSpamTableSeeder::class);
        $this->call(TblSpamBudgetsTableSeeder::class);
        $this->call(TblPengelolaTableSeeder::class);

        \Illuminate\Support\Facades\Schema::enableForeignKeyConstraints();
        $this->call(TblSpamAchievementsTableSeeder::class);
    }
}
