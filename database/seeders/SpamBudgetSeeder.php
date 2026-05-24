<?php

namespace Database\Seeders;

use Illuminate\Database\Console\Seeds\WithoutModelEvents;
use Illuminate\Database\Seeder;

class SpamBudgetSeeder extends Seeder
{
    /**
     * Run the database seeds.
     */
    public function run(): void
    {
        $csvPath = 'C:\Users\asusg\Downloads\spse_cianjur_keduanya_2013-2025_spam_wilayah.csv';
        if (file_exists($csvPath)) {
            \Illuminate\Support\Facades\Artisan::call('spam:import-data');
            $this->command->info(\Illuminate\Support\Facades\Artisan::output());
        } else {
            $this->command->error("CSV file not found at: {$csvPath}");
        }
    }
}
