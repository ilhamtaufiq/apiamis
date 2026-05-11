<?php
require 'vendor/autoload.php';
$app = require_once 'bootstrap/app.php';
$app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();

$admin = \App\Models\User::whereHas('roles', function($q) { $q->where('name', 'admin'); })->first();
auth()->login($admin);

use App\Models\Pekerjaan;

$query = "Berapa banyak paket Pembangunan Sistem Pengelolaan Air Limbah Domestik (SPALD) Terpusat Skala Permukiman di 2025?";
$queryLower = strtolower($query);
$stopWords = ['apa', 'bagaimana', 'siapa', 'dimana', 'kapan', 'tampilkan', 'lihat', 'cari', 'dong', 'sih', 'ya', 'kah', 'tolong', 'bisa', 'boleh', 'yang', 'di', 'ke', 'dari', 'dan', 'atau', 'berapa', 'banyak', 'jumlah', 'total', 'paket', 'pekerjaan', 'tahun', 'anggaran'];
$regex = '/\b(' . implode('|', $stopWords) . ')\b/u';
$cleanQuery = preg_replace($regex, '', $queryLower);
$cleanQuery = preg_replace('/[^\w\s]/u', '', $cleanQuery);
$cleanQuery = trim(preg_replace('/\s+/', ' ', $cleanQuery));

$searchQuery = $cleanQuery;
$year = null;
if (preg_match('/\b(20\d{2})\b/', $query, $matches)) {
    $year = $matches[1];
    $searchQuery = trim(str_replace($year, '', $searchQuery));
}

echo "Search Query: '$searchQuery'\n";
echo "Year: '$year'\n";

$statsQuery = Pekerjaan::byUserRole();
if ($searchQuery || $year) {
    $statsQuery->where(function ($q) use ($searchQuery, $year) {
        if ($searchQuery) {
            $keywords = explode(' ', $searchQuery);
            $q->where(function($subQ) use ($keywords) {
                foreach ($keywords as $word) {
                    if (strlen($word) < 2) continue;
                    $subQ->where(function($finalQ) use ($word) {
                        $finalQ->where('nama_paket', 'LIKE', "%{$word}%")
                              ->orWhere('kode_rekening', 'LIKE', "%{$word}%")
                              ->orWhereHas('kegiatan', function($k) use ($word) {
                                  $k->where('nama_sub_kegiatan', 'LIKE', "%{$word}%");
                              });
                    });
                }
            });
        }
        
        if ($year) {
            $q->whereHas('kegiatan', function($sub) use ($year) {
                $sub->where('tahun_anggaran', $year);
            });
        }
    });
}

$count = $statsQuery->count();
echo "TOTAL_COUNT: $count\n";
