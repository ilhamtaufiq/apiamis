<?php

namespace Database\Seeders;

use App\Models\Blog;
use App\Models\User;
use Illuminate\Database\Seeder;
use Illuminate\Support\Str;

class BlogSeeder extends Seeder
{
    /**
     * Run the database seeds.
     */
    public function run(): void
    {
        $admin = User::role('admin')->first();
        
        if (!$admin) {
            $admin = User::first();
        }

        if (!$admin) return;

        $posts = [
            [
                'title' => 'Transformasi Digital Infrastruktur Jabar: Menuju Smart Province 2030',
                'content' => '<p>Bagaimana integrasi teknologi IoT dan AI membantu memantau kondisi jalan dan jembatan secara real-time di seluruh wilayah Jawa Barat.</p><p>Pembangunan infrastruktur digital di Jawa Barat terus mengalami akselerasi signifikan. Melalui platform Arumanis, integrasi data pekerjaan lapangan kini dapat dilakukan secara real-time, memungkinkan pengambilan keputusan yang lebih cepat dan akurat.</p><h2>Inovasi Berkelanjutan</h2><p>Salah satu fokus utama tahun ini adalah implementasi sistem monitoring berbasis IoT pada proyek-proyek jembatan strategis. Sensor-sensor yang terpasang akan mengirimkan data beban dan getaran langsung ke pusat kontrol, memberikan peringatan dini jika terdeteksi anomali pada struktur bangunan.</p><blockquote>"Teknologi bukan hanya alat, melainkan fondasi bagi pembangunan yang lebih efisien dan transparan."</blockquote>',
                'category' => 'Teknologi',
                'cover_image' => 'https://images.unsplash.com/photo-1486406146926-c627a92ad1ab?q=80&w=800&auto=format&fit=crop',
                'is_published' => true,
                'published_at' => now(),
            ],
            [
                'title' => 'Pembangunan Jembatan Gantung Simpang Dago Selesai Lebih Cepat',
                'content' => '<p>Proyek strategis jembatan gantung di wilayah Dago telah mencapai tahap finalisasi. Inovasi material beton ringan menjadi kunci percepatan.</p><p>Tim teknis di lapangan melaporkan bahwa penggunaan teknologi prefabrikasi modern memungkinkan struktur utama jembatan berdiri hanya dalam waktu 3 bulan, jauh lebih cepat dari estimasi awal 6 bulan.</p>',
                'category' => 'Infrastruktur',
                'cover_image' => 'https://images.unsplash.com/photo-1545139139-1bc901a9b9a6?q=80&w=800&auto=format&fit=crop',
                'is_published' => true,
                'published_at' => now()->subDays(2),
            ],
            [
                'title' => 'Mengenal Sistem Monitoring Banjir Berbasis Cloud di Kota Bandung',
                'content' => '<p>Dinas Pekerjaan Umum meluncurkan dashboard publik untuk memantau ketinggian air sungai sebagai langkah antisipasi bencana.</p><p>Sistem ini mengintegrasikan ribuan sensor level air yang tersebar di sepanjang sungai-sungai utama di Bandung, memberikan peringatan dini ke smartphone warga melalui aplikasi terintegrasi.</p>',
                'category' => 'Berita',
                'cover_image' => 'https://images.unsplash.com/photo-1558441139-444167909302?q=80&w=800&auto=format&fit=crop',
                'is_published' => true,
                'published_at' => now()->subDays(5),
            ]
        ];

        foreach ($posts as $post) {
            Blog::create([
                'title' => $post['title'],
                'slug' => Str::slug($post['title']) . '-' . Str::random(5),
                'content' => $post['content'],
                'category' => $post['category'],
                'cover_image' => $post['cover_image'],
                'user_id' => $admin->id,
                'is_published' => $post['is_published'],
                'published_at' => $post['published_at'],
            ]);
        }
    }
}
