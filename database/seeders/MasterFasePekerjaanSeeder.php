<?php

namespace Database\Seeders;

use Illuminate\Database\Console\Seeds\WithoutModelEvents;
use Illuminate\Database\Seeder;

class MasterFasePekerjaanSeeder extends Seeder
{
    /**
     * Run the database seeds.
     */
    public function run(): void
    {
        $fases = [
            // SANITASI
            [
                'jenis_proyek' => 'sanitasi',
                'kode_fase' => 'persiapan',
                'nama_fase' => 'Pekerjaan Persiapan',
                'prioritas' => 0,
                'overlap_persen' => 0,
                'keywords' => json_encode(['persiapan', 'mobilisasi', 'bouwplank', 'pengukuran']),
            ],
            [
                'jenis_proyek' => 'sanitasi',
                'kode_fase' => 'tanah',
                'nama_fase' => 'Pekerjaan Tanah',
                'prioritas' => 1,
                'overlap_persen' => 0,
                'keywords' => json_encode(['tanah', 'galian', 'urugan', 'timbunan']),
            ],
            [
                'jenis_proyek' => 'sanitasi',
                'kode_fase' => 'bangunan_pengolah',
                'nama_fase' => 'Bangunan Pengolah',
                'prioritas' => 2,
                'overlap_persen' => 20,
                'keywords' => json_encode(['stp', 'biofilter', 'septik', 'bak', 'sump pit', 'grease trap', 'sumur resapan', 'struktur risha', 'beton', 'bekisting']),
            ],
            [
                'jenis_proyek' => 'sanitasi',
                'kode_fase' => 'perpipaan_dalam',
                'nama_fase' => 'Perpipaan Dalam Gedung',
                'prioritas' => 3,
                'overlap_persen' => 25,
                'keywords' => json_encode(['perpipaan dalam', 'pipa pvc', 'pipa galvanis', 'pipa ppr', 'pipa hdpe']),
            ],
            [
                'jenis_proyek' => 'sanitasi',
                'kode_fase' => 'perpipaan_luar',
                'nama_fase' => 'Perpipaan Luar Gedung',
                'prioritas' => 4,
                'overlap_persen' => 20,
                'keywords' => json_encode(['perpipaan luar', 'jaringan pipa', 'pipa dci', 'pipa beton', 'pipa baja']),
            ],
            [
                'jenis_proyek' => 'sanitasi',
                'kode_fase' => 'aksesoris',
                'nama_fase' => 'Aksesoris & Valve',
                'prioritas' => 5,
                'overlap_persen' => 30,
                'keywords' => json_encode(['aksesoris', 'valve', 'tee', 'reducer', 'bend', 'clamp', 'sambungan']),
            ],
            [
                'jenis_proyek' => 'sanitasi',
                'kode_fase' => 'testing',
                'nama_fase' => 'Testing & Commissioning',
                'prioritas' => 6,
                'overlap_persen' => 0,
                'keywords' => json_encode(['testing', 'commissioning', 'uji', 'flushing']),
            ],
            // AIR MINUM (SPAM)
            [
                'jenis_proyek' => 'air_minum',
                'kode_fase' => 'persiapan',
                'nama_fase' => 'Pekerjaan Persiapan',
                'prioritas' => 0,
                'overlap_persen' => 0,
                'keywords' => json_encode(['persiapan', 'mobilisasi', 'bouwplank', 'pengukuran']),
            ],
            [
                'jenis_proyek' => 'air_minum',
                'kode_fase' => 'sumber_air',
                'nama_fase' => 'Pekerjaan Sumber Air',
                'prioritas' => 1,
                'overlap_persen' => 0,
                'keywords' => json_encode(['pemboran', 'sumur', 'mata air', 'intake', 'broncaptering', 'pilot hole', 'reaming', 'cassing']),
            ],
            [
                'jenis_proyek' => 'air_minum',
                'kode_fase' => 'bangunan_penampung',
                'nama_fase' => 'Bangunan Penampung',
                'prioritas' => 2,
                'overlap_persen' => 20,
                'keywords' => json_encode(['reservoir', 'tangki', 'penampung', 'ground tank', 'tower', 'hidropore', 'beton', 'pembesian']),
            ],
            [
                'jenis_proyek' => 'air_minum',
                'kode_fase' => 'bangunan_pelengkap',
                'nama_fase' => 'Bangunan Pelengkap',
                'prioritas' => 3,
                'overlap_persen' => 25,
                'keywords' => json_encode(['rumah pompa', 'rumah jaga', 'bangunan bagi', 'bak pelepas']),
            ],
            [
                'jenis_proyek' => 'air_minum',
                'kode_fase' => 'perpipaan_transmisi',
                'nama_fase' => 'Perpipaan Transmisi',
                'prioritas' => 4,
                'overlap_persen' => 25,
                'keywords' => json_encode(['transmisi', 'pipa pvc', 'pipa gi', 'pipa hdpe', 'galian pipa']),
            ],
            [
                'jenis_proyek' => 'air_minum',
                'kode_fase' => 'perpipaan_distribusi',
                'nama_fase' => 'Perpipaan Distribusi',
                'prioritas' => 5,
                'overlap_persen' => 30,
                'keywords' => json_encode(['distribusi', 'jaringan', 'reducer', 'tee', 'bend', 'valve', 'gate valve']),
            ],
            [
                'jenis_proyek' => 'air_minum',
                'kode_fase' => 'sambungan_rumah',
                'nama_fase' => 'Sambungan Rumah',
                'prioritas' => 6,
                'overlap_persen' => 30,
                'keywords' => json_encode(['sambungan rumah', 'sr', 'water meter', 'kran', 'hidran umum', 'box sr']),
            ],
            [
                'jenis_proyek' => 'air_minum',
                'kode_fase' => 'testing',
                'nama_fase' => 'Testing & Commissioning',
                'prioritas' => 7,
                'overlap_persen' => 0,
                'keywords' => json_encode(['testing', 'uji', 'flushing', 'desinfeksi']),
            ],
        ];

        foreach ($fases as $fase) {
            \App\Models\MasterFasePekerjaan::updateOrCreate(
                [
                    'jenis_proyek' => $fase['jenis_proyek'],
                    'kode_fase' => $fase['kode_fase']
                ],
                $fase
            );
        }
    }
}
