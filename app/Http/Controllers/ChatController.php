<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use App\Services\OpenRouterService;
use App\Models\Pekerjaan;
use App\Models\Kontrak;
use App\Models\Penyedia;
use App\Models\Kegiatan;
use App\Models\Desa;
use Illuminate\Support\Facades\DB;

class ChatController extends Controller
{
    protected $openRouter;

    public function __construct(OpenRouterService $openRouter)
    {
        $this->openRouter = $openRouter;
    }

    /**
     * Handle AI Chat with Database Context
     */
    public function chat(Request $request)
    {
        $request->validate([
            'message' => 'required|string',
            'history' => 'nullable|array',
        ]);

        $userMessage = $request->input('message');
        $history = $request->input('history', []);

        // 1. Optimize History: Keep only last 10 messages to save tokens
        $maxHistory = 10;
        if (count($history) > $maxHistory) {
            $history = array_slice($history, -$maxHistory);
        }

        // 2. Get Context from Database based on user message
        $context = $this->getDatabaseContext($userMessage);

        // 3. Prepare Messages for AI
        $messages = [];
        
        // System Prompt with Schema Knowledge
        $messages[] = [
            'role' => 'system',
            'content' => "Anda adalah 'Ami', asisten AI analisis data untuk aplikasi Arumanis (Sistem Informasi Pekerjaan Umum). 
            Tugas utama Anda adalah membantu pengguna MENCARI dan MENGANALISIS data proyek (Pekerjaan), Kontrak, Penyedia (Kontraktor), dan Kegiatan berdasarkan konteks database yang diberikan.

            PENTING: Anda adalah asisten 'Read-Only'. Anda TIDAK memiliki kemampuan untuk menginput, mengubah, atau menghapus data dalam sistem. Tugas Anda hanya memberikan informasi berdasarkan data yang ada.

            PANDUAN VISUAL & INTERAKSI:
            1. Jika konteks data menyediakan URL foto, tampilkan foto tersebut menggunakan format Markdown: ![Keterangan](URL).
            2. Berikan informasi progres fisik secara jelas. Sebutkan RATA-RATA PROGRES jika ditanya tentang wilayah atau kategori.
            3. Sertakan LINK DETAIL proyek (jika ada dalam konteks) agar pengguna bisa langsung membuka halaman tersebut.
            4. Selalu akhiri jawaban dengan 1 saran PERTANYAAN LANJUTAN yang relevan (misal: 'Apakah Anda ingin melihat detail penyedia untuk proyek ini?').

            Berikut adalah konteks data relasional dari database yang sesuai dengan pertanyaan pengguna:\n\n" . $context . "\n\n
            PANDUAN JAWABAN:
            1. Jika pengguna bertanya tentang siapa yang mengerjakan proyek, lihat data Kontrak -> Penyedia.
            2. Jika pengguna bertanya tentang lokasi, lihat data Desa/Kecamatan di Pekerjaan.
            3. Analisis relasi antar data untuk memberikan jawaban yang komprehensif.
            4. Gunakan data di atas untuk menjawab secara detail dan informatif.
            5. Jika ada foto, tampilkan foto tersebut untuk membantu visualisasi.
            Bahasa: Indonesia."
        ];

        // Add pruned history
        foreach ($history as $msg) {
            // Ensure each message has role and content
            if (isset($msg['role']) && isset($msg['content'])) {
                $messages[] = [
                    'role' => $msg['role'],
                    'content' => $msg['content']
                ];
            }
        }

        // Add current user message
        $messages[] = [
            'role' => 'user',
            'content' => $userMessage
        ];

        // 4. Call AI
        $result = $this->openRouter->chat($messages, [
            'model' => 'nvidia/nemotron-3-super-120b-a12b:free',
        ]);

        if (!$result['success']) {
            return response()->json($result, 500);
        }

        return response()->json([
            'success' => true,
            'reply' => $result['content'],
            'usage' => $result['usage'] ?? null
        ]);
    }

    /**
     * Relational RAG: Search database with role-based security and detailed context
     */
    private function getDatabaseContext($query)
    {
        $context = "";
        $queryLower = strtolower($query);

        // 1. STATISTIK & RINGKASAN (Untuk menjawab pertanyaan "Berapa banyak", "Total", dll)
        $statsQuery = Pekerjaan::byUserRole()
            ->where(function ($q) use ($query) {
                $q->where('nama_paket', 'LIKE', "%{$query}%")
                  ->orWhere('kode_rekening', 'LIKE', "%{$query}%")
                  ->orWhereHas('desa', function($sub) use ($query) { $sub->where('n_desa', 'LIKE', "%{$query}%"); })
                  ->orWhereHas('kecamatan', function($sub) use ($query) { $sub->where('n_kec', 'LIKE', "%{$query}%"); })
                  ->orWhereHas('kegiatan', function($sub) use ($query) { 
                      $sub->where('nama_sub_kegiatan', 'LIKE', "%{$query}%")
                          ->orWhere('tahun_anggaran', 'LIKE', "%{$query}%");
                  });
            });

        $totalCount = $statsQuery->count();
        if ($totalCount > 0) {
            $avgProgress = $statsQuery->with('progress')->get()->avg(function($p) {
                return $p->progress->realisasi ?? 0;
            });

            $context .= "### STATISTIK & RINGKASAN:\n";
            $context .= "- Total Pekerjaan Ditemukan: {$totalCount}\n";
            $context .= "- Rata-rata Progres Fisik: " . number_format($avgProgress, 2) . "%\n";
            
            // Jika ada query tahun, berikan breakdown per sub kegiatan
            if (preg_match('/\b(20\d{2})\b/', $query, $matches)) {
                $year = $matches[1];
                $breakdown = Pekerjaan::byUserRole()
                    ->whereHas('kegiatan', function($q) use ($year) { $q->where('tahun_anggaran', $year); })
                    ->select('kegiatan_id', DB::raw('count(*) as total'))
                    ->groupBy('kegiatan_id')
                    ->with('kegiatan')
                    ->get();
                
                if ($breakdown->count() > 0) {
                    $context .= "- Breakdown Tahun Anggaran {$year}:\n";
                    foreach ($breakdown as $b) {
                        $context .= "  * " . ($b->kegiatan->nama_sub_kegiatan ?? 'Lainnya') . ": {$b->total} paket\n";
                    }
                }
            }
            $context .= "\n";
        }

        // Base URL for links (Adjust according to frontend domain)
        $baseUrl = config('app.frontend_url', url('/'));

        // 2. Search Pekerjaan with its relations (Detailed List)
        $pekerjaan = Pekerjaan::byUserRole()
            ->with(['kecamatan', 'desa', 'kontrak.penyedia', 'kegiatan', 'progress', 'pengawas', 'foto'])
            ->where(function ($q) use ($query) {
                // Hybrid Search
                $q->whereRaw("MATCH(nama_paket, kode_rekening) AGAINST(? IN NATURAL LANGUAGE MODE)", [$query])
                  ->orWhere('nama_paket', 'LIKE', "%{$query}%")
                  ->orWhereHas('desa', function($sub) use ($query) { $sub->where('n_desa', 'LIKE', "%{$query}%"); })
                  ->orWhereHas('kecamatan', function($sub) use ($query) { $sub->where('n_kec', 'LIKE', "%{$query}%"); })
                  ->orWhereHas('kegiatan', function($sub) use ($query) { 
                      $sub->where('nama_sub_kegiatan', 'LIKE', "%{$query}%")
                          ->orWhere('tahun_anggaran', 'LIKE', "%{$query}%");
                  });
            })
            ->limit(5)->get();

        if ($pekerjaan->count() > 0) {
            $context .= "### DAFTAR DETAIL PEKERJAAN:\n";
            foreach ($pekerjaan as $p) {
                $loc = ($p->desa->n_desa ?? '-') . ", " . ($p->kecamatan->n_kec ?? '-');
                $progres = $p->progress->realisasi ?? 0;
                $pengawas = $p->pengawas->nama ?? 'Belum ditunjuk';
                
                $context .= "- Paket: [ID: {$p->id}] {$p->nama_paket}\n";
                $context .= "  * Link Detail: {$baseUrl}/pekerjaan/{$p->id}\n";
                $context .= "  * Pagu: Rp " . number_format($p->pagu, 0, ',', '.') . "\n";
                $context .= "  * Lokasi: {$loc}\n";
                $context .= "  * Progres Fisik: {$progres}%\n";
                $context .= "  * Pengawas Lapangan: {$pengawas}\n";
                $context .= "  * Sub Kegiatan: " . ($p->kegiatan->nama_sub_kegiatan ?? '-') . " (" . ($p->kegiatan->tahun_anggaran ?? '-') . ")\n";
                
                // Add Photo URLs to context
                if ($p->foto->count() > 0) {
                    $context .= "  * Foto Lapangan:\n";
                    foreach ($p->foto->take(3) as $f) {
                        $url = $f->getFirstMediaUrl('foto/pekerjaan');
                        if ($url) {
                            $context .= "    - URL: {$url} (Keterangan: " . ($f->keterangan ?? 'Foto Proyek') . ")\n";
                        }
                    }
                }

                if ($p->kontrak->count() > 0) {
                    $context .= "  * Detail Kontrak:\n";
                    foreach ($p->kontrak as $k) {
                        $context .= "    - No SPK: {$k->spk} | Penyedia: " . ($k->penyedia->nama ?? 'N/A') . " (Nilai: Rp " . number_format($k->nilai_kontrak, 0, ',', '.') . ")\n";
                    }
                }
                $context .= "\n";
            }
        }

        // 2. Search Penyedia/Kontraktor directly if mentioned
        if (str_contains($queryLower, 'kontraktor') || str_contains($queryLower, 'penyedia') || $pekerjaan->count() < 2) {
            $penyedia = Penyedia::where(function ($q) use ($query) {
                $q->whereRaw("MATCH(nama, direktur) AGAINST(? IN NATURAL LANGUAGE MODE)", [$query])
                  ->orWhere('nama', 'LIKE', "%{$query}%");
            })
            ->limit(3)->get();

            if ($penyedia->count() > 0) {
                $context .= "### DATA PENYEDIA (KONTRAKTOR):\n";
                foreach ($penyedia as $pen) {
                    $context .= "- Nama: {$pen->nama} (Direktur: {$pen->direktur})\n";
                    
                    // Get latest projects for this contractor (filtered by user role)
                    $kontrakTerbaru = Kontrak::where('id_penyedia', $pen->id)
                        ->whereHas('pekerjaan', function($q) { $q->byUserRole(); })
                        ->with('pekerjaan')
                        ->latest()->limit(2)->get();

                    if ($kontrakTerbaru->count() > 0) {
                        $context .= "  * Proyek yang sedang/pernah dikerjakan:\n";
                        foreach ($kontrakTerbaru as $k) {
                            $context .= "    - " . ($k->pekerjaan->nama_paket ?? 'N/A') . " (SPK: {$k->spk})\n";
                        }
                    }
                    $context .= "\n";
                }
            }
        }

        // 3. Fallback: Recent relevant data if context still minimal
        if (strlen($context) < 100) {
            $recent = Pekerjaan::byUserRole()
                ->with(['kontrak.penyedia', 'progress'])
                ->latest()->limit(3)->get();
                
            if ($recent->count() > 0) {
                $context .= "### DATA TERBARU (Mungkin Relevan):\n";
                foreach ($recent as $r) {
                    $penyediaName = $r->kontrak->first()->penyedia->nama ?? 'Belum ada penyedia';
                    $progres = $r->progress->realisasi ?? 0;
                    $context .= "- {$r->nama_paket} | Pagu: Rp " . number_format($r->pagu, 0, ',', '.') . " | Progres: {$progres}% | Penyedia: {$penyediaName}\n";
                }
            }
        }

        return $context;
    }
}
