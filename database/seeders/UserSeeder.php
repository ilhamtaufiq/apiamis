<?php

namespace Database\Seeders;

use Illuminate\Database\Console\Seeds\WithoutModelEvents;
use Illuminate\Database\Seeder;

class UserSeeder extends Seeder
{
    /**
     * Run the database seeds.
     */
    public function run(): void
    {
        \App\Models\User::updateOrCreate(
            ['email' => 'admin@apiamis.test'],
            [
                'name' => 'Admin ApiAmis',
                'password' => \Illuminate\Support\Facades\Hash::make('password123'),
                'email_verified_at' => now(),
            ]
        );
    }
}
