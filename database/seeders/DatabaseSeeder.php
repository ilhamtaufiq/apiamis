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
        // Disable FK checks so seeders can insert in any order
        \Illuminate\Support\Facades\Schema::disableForeignKeyConstraints();

        $this->call(TblUnitSpamTableSeeder::class);
        $this->call(TblSpamBudgetsTableSeeder::class);
        $this->call(TblPengelolaTableSeeder::class);
        $this->call(TblSpamAchievementsTableSeeder::class);

        \Illuminate\Support\Facades\Schema::enableForeignKeyConstraints();
    }
}
