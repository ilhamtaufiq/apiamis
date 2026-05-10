<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use App\Services\OpenRouterService;
use App\Models\Pekerjaan;
use App\Models\Kontrak;
use App\Models\Penyedia;
use App\Models\Kegiatan;
use App\Models\Desa;
use App\Models\ChatSession;
use App\Models\ChatMessage;
use App\Models\ChatKnowledgeCache;
use App\Models\Foto;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Cache;

class ChatController extends Controller
{
    protected $openRouter;

    public function __construct(OpenRouterService $openRouter)
    {
        $this->openRouter = $openRouter;
    }

    // ── Session CRUD ────────────────────────────────────────────────

    /**
     * List user's chat sessions
     */
    public function sessions(Request $request)
    {
        $sessions = ChatSession::where('user_id', $request->user()->id)
            ->withCount('messages')
            ->orderByDesc('updated_at')
            ->limit(50)
            ->get()
            ->map(fn($s) => [
                'id' => $s->id,
                'title' => $s->title,
                'messages_count' => $s->messages_count,
                'updated_at' => $s->updated_at->diffForHumans(),
                'updated_at_raw' => $s->updated_at,
            ]);

        return response()->json(['success' => true, 'data' => $sessions]);
    }

    /**
     * Create a new session
     */
    public function createSession(Request $request)
    {
        $session = ChatSession::create([
            'user_id' => $request->user()->id,
            'title' => 'Percakapan Baru',
        ]);

        return response()->json(['success' => true, 'data' => $session]);
    }

    /**
     * Delete a session
     */
    public function deleteSession(Request $request, $id)
    {
        $session = ChatSession::where('user_id', $request->user()->id)->findOrFail($id);
        $session->delete();

        return response()->json(['success' => true]);
    }

    /**
     * Get messages for a session
     */
    public function sessionMessages(Request $request, $id)
    {
        $session = ChatSession::where('user_id', $request->user()->id)->findOrFail($id);

        return response()->json([
            'success' => true,
            'data' => [
                'session' => $session,
                'messages' => $session->messages()->get()->map(fn($m) => [
                    'role' => $m->role,
                    'content' => $m->content,
                    'tool_calls' => $m->tool_calls,
                    'created_at' => $m->created_at,
                ]),
            ],
        ]);
    }

    // ── Main Chat (with cache + learning) ───────────────────────────

    /**
     * Handle AI Chat with Database Context, Caching, and Learning
     */
    public function chat(Request $request)
    {
        $request->validate([
            'message' => 'required|string',
            'session_id' => 'nullable|integer',
            'history' => 'nullable|array',
        ]);

        $userMessage = $request->input('message');
        $sessionId = $request->input('session_id');
        $history = $request->input('history', []);
        $user = $request->user();

        // ── 0. Session management ───────────────────────────────
        $session = null;
        if ($sessionId) {
            $session = ChatSession::where('user_id', $user->id)->find($sessionId);
        }
        if (!$session) {
            $session = ChatSession::create([
                'user_id' => $user->id,
                'title' => 'Percakapan Baru',
            ]);
        }

        // Save user message to DB
        ChatMessage::create([
            'chat_session_id' => $session->id,
            'role' => 'user',
            'content' => $userMessage,
        ]);

        // ── 1. Check knowledge cache (skip AI call if cached) ───
        $cached = ChatKnowledgeCache::findSimilar($userMessage);
        if ($cached && strlen($cached->response) > 50) {
            // Save cached response as assistant message
            ChatMessage::create([
                'chat_session_id' => $session->id,
                'role' => 'assistant',
                'content' => $cached->response,
                'tokens_used' => 0, // No tokens used!
            ]);

            // Auto-title on first message
            if ($session->messages()->count() <= 2) {
                $session->generateTitle();
            }

            return response()->json([
                'success' => true,
                'reply' => $cached->response,
                'session_id' => $session->id,
                'cached' => true,
                'usage' => ['total_tokens' => 0],
            ]);
        }

        // ── 2. Optimize History from DB (not from client) ───────
        $dbHistory = $session->messages()
            ->orderByDesc('id')
            ->limit(10)
            ->get()
            ->reverse()
            ->values();

        // ── 3. Get Context from Database (with Laravel Cache) ───
        $contextCacheKey = 'chat_ctx_' . md5($userMessage) . '_' . $user->id;
        $context = Cache::remember($contextCacheKey, 300, function () use ($userMessage) {
            return $this->getDatabaseContext($userMessage);
        });

        // ── 4. Inject few-shot examples from knowledge cache ────
        $fewShot = ChatKnowledgeCache::getFewShotExamples(2);
        $fewShotText = '';
        if (!empty($fewShot)) {
            $fewShotText = "\n\nCONTOH JAWABAN YANG BAIK (untuk referensi gaya):\n";
            foreach ($fewShot as $i => $ex) {
                $fewShotText .= "Q: {$ex['query']}\nA: {$ex['response']}\n---\n";
            }
        }

        // ── 5. Prepare Messages for AI ──────────────────────────
        $messages = [];
        
        // System Prompt (compressed)
        $messages[] = [
            'role' => 'system',
            'content' => "Anda adalah 'Ami', asisten AI analisis data untuk aplikasi Arumanis (Sistem Informasi Pekerjaan Umum). 
            Tugas utama Anda adalah membantu pengguna MENCARI dan MENGANALISIS data proyek (Pekerjaan), Kontrak, Penyedia (Kontraktor), dan Kegiatan berdasarkan konteks database yang diberikan.

            PENTING: Anda adalah asisten 'Read-Only'. Anda TIDAK memiliki kemampuan untuk menginput, mengubah, atau menghapus data dalam sistem. Tugas Anda hanya memberikan informasi berdasarkan data yang ada.

            PANDUAN VISUAL & INTERAKSI:
            1. Gunakan Markdown secara maksimal. 
            2. Jika menyajikan daftar data (Pekerjaan, Kontrak, atau Penyedia) yang lebih dari 2 item, WAJIB menggunakan tabel Markdown agar tampilan rapi.
            3. Jika konteks data menyediakan URL foto, tampilkan foto tersebut menggunakan format Markdown: ![Keterangan](URL).
            4. Berikan informasi progres fisik secara jelas. Sebutkan RATA-RATA PROGRES jika ditanya tentang wilayah atau kategori.
            5. Sertakan LINK DETAIL proyek (jika ada dalam konteks) agar pengguna bisa langsung membuka halaman tersebut. Gunakan format [Nama Paket](URL).
            6. SKILL KHUSUS: Anda memiliki kemampuan 'Pencarian Foto'. Jika pengguna meminta foto, cari di bagian 'FOTO LAPANGAN' atau 'HASIL PENCARIAN FOTO' dalam konteks. Tampilkan menggunakan Markdown.
            7. Selalu akhiri jawaban dengan 1 saran PERTANYAAN LANJUTAN yang relevan (misal: 'Apakah Anda ingin melihat detail penyedia untuk proyek ini?').
 
            Berikut adalah konteks data relasional dari database yang sesuai dengan pertanyaan pengguna:\n\n" . $context . "\n\n" . $fewShotText . "
            PANDUAN JAWABAN:
            1. Jika pengguna bertanya tentang siapa yang mengerjakan proyek, lihat data Kontrak -> Penyedia.
            2. Jika pengguna bertanya tentang lokasi, lihat data Desa/Kecamatan di Pekerjaan.
            3. Analisis relasi antar data untuk memberikan jawaban yang komprehensif.
            4. Gunakan data di atas untuk menjawab secara detail dan informatif.
            5. Jika ada foto, tampilkan foto tersebut untuk membantu visualisasi.
            6. Jika data yang diminta banyak, rangkum dalam tabel yang kolomnya relevan (misal: No, Paket, Lokasi, Pagu, Progres).
            Bahasa: Indonesia."
        ];

        // Add DB history (compressed: only content, skip tool_calls)
        foreach ($dbHistory as $msg) {
            if ($msg->role && $msg->content) {
                $content = $msg->content;
                // Compress old assistant messages to save tokens
                if ($msg->role === 'assistant' && mb_strlen($content) > 500 && $msg->id !== $dbHistory->last()->id) {
                    $content = mb_substr($content, 0, 400) . "\n...(dipotong untuk efisiensi)";
                }
                $messages[] = [
                    'role' => $msg->role,
                    'content' => $content,
                ];
            }
        }

        // ── 6. Call AI ──────────────────────────────────────────
        $result = $this->openRouter->chat($messages);

        if (!$result['success']) {
            return response()->json($result, 500);
        }

        $aiReply = $result['content'];
        $tokensUsed = $result['usage']['total_tokens'] ?? 0;

        // ── 7. Save assistant response to DB ────────────────────
        ChatMessage::create([
            'chat_session_id' => $session->id,
            'role' => 'assistant',
            'content' => $aiReply,
            'tool_calls' => $result['tool_calls'] ?? null,
            'tokens_used' => $tokensUsed,
        ]);

        // ── 8. Learn from this interaction (cache for future) ───
        ChatKnowledgeCache::learn($userMessage, $context, $aiReply, $tokensUsed);

        // ── 9. Auto-title on first exchange ─────────────────────
        if ($session->messages()->count() <= 2) {
            $session->generateTitle();
        }

        // Touch session updated_at
        $session->touch();

        return response()->json([
            'success' => true,
            'reply' => $aiReply,
            'session_id' => $session->id,
            'cached' => false,
            'usage' => $result['usage'] ?? null,
        ]);
    }

    /**
     * Relational RAG: Search database with role-based security and detailed context
     */
    private function getDatabaseContext($query)
    {
        $context = "";
        $queryLower = strtolower($query);
        $isSearchingPhoto = str_contains($queryLower, 'foto') || str_contains($queryLower, 'gambar') || str_contains($queryLower, 'dokumentasi') || str_contains($queryLower, 'lihat');

        // Clean query from common keywords for better database matching
        $cleanQuery = str_replace(['foto', 'gambar', 'dokumentasi', 'lihat', 'cari', 'tampilkan'], '', $queryLower);
        $cleanQuery = trim($cleanQuery);
        $searchQuery = $cleanQuery ?: $query; // Fallback to original if empty after cleaning

        // 1. STATISTIK & RINGKASAN (Untuk menjawab pertanyaan "Berapa banyak", "Total", dll)
        $statsQuery = Pekerjaan::byUserRole()
            ->where(function ($q) use ($searchQuery) {
                $q->where('nama_paket', 'LIKE', "%{$searchQuery}%")
                  ->orWhere('kode_rekening', 'LIKE', "%{$searchQuery}%")
                  ->orWhereHas('desa', function($sub) use ($searchQuery) { $sub->where('n_desa', 'LIKE', "%{$searchQuery}%"); })
                  ->orWhereHas('kecamatan', function($sub) use ($searchQuery) { $sub->where('n_kec', 'LIKE', "%{$searchQuery}%"); })
                  ->orWhereHas('kegiatan', function($sub) use ($searchQuery) { 
                      $sub->where('nama_sub_kegiatan', 'LIKE', "%{$searchQuery}%")
                          ->orWhere('tahun_anggaran', 'LIKE', "%{$searchQuery}%");
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

        // 1.1 KHUSUS PENCARIAN FOTO (Skill: Foto Search)
        if ($isSearchingPhoto) {
            $photoSearch = Pekerjaan::byUserRole()
                ->whereHas('foto')
                ->with('foto')
                ->where(function($q) use ($searchQuery) {
                    $q->where('nama_paket', 'LIKE', "%{$searchQuery}%")
                      ->orWhereHas('desa', function($sub) use ($searchQuery) { $sub->where('n_desa', 'LIKE', "%{$searchQuery}%"); });
                })
                ->limit(5)->get();

            if ($photoSearch->count() > 0) {
                $context .= "### HASIL PENCARIAN FOTO PEKERJAAN:\n";
                foreach ($photoSearch as $ps) {
                    $context .= "- Paket: {$ps->nama_paket}\n";
                    foreach ($ps->foto as $f) {
                        $url = $f->getFirstMediaUrl('foto/pekerjaan');
                        if ($url) {
                            $context .= "  * ![{$f->keterangan}]({$url})\n";
                            $context .= "    (Keterangan: " . ($f->keterangan ?? 'Foto Lapangan') . ")\n";
                        }
                    }
                }
                $context .= "\n";
            } else if (str_contains($queryLower, 'terbaru')) {
                $latestPhotos = Foto::whereHas('pekerjaan', function($q) { $q->byUserRole(); })
                    ->with('pekerjaan')
                    ->latest()
                    ->limit(5)
                    ->get();

                if ($latestPhotos->count() > 0) {
                    $context .= "### FOTO TERBARU DIUPLOAD:\n";
                    /** @var Foto $lp */
                    foreach ($latestPhotos as $lp) {
                        $url = $lp->getFirstMediaUrl('foto/pekerjaan');
                        if ($url) {
                            $context .= "- Proyek: " . ($lp->pekerjaan->nama_paket ?? 'N/A') . "\n";
                            $context .= "  * ![Foto Terbaru]({$url})\n";
                        }
                    }
                    $context .= "\n";
                }
            }
        }

        // Base URL for links (Adjust according to frontend domain)
        $baseUrl = config('app.frontend_url', url('/'));

        // 2. Search Pekerjaan with its relations (Detailed List)
        $pekerjaan = Pekerjaan::byUserRole()
            ->with(['kecamatan', 'desa', 'kontrak.penyedia', 'kegiatan', 'progress', 'pengawas', 'foto'])
            ->where(function ($q) use ($searchQuery) {
                // Hybrid Search
                $q->whereRaw("MATCH(nama_paket, kode_rekening) AGAINST(? IN NATURAL LANGUAGE MODE)", [$searchQuery])
                  ->orWhere('nama_paket', 'LIKE', "%{$searchQuery}%")
                  ->orWhereHas('desa', function($sub) use ($searchQuery) { $sub->where('n_desa', 'LIKE', "%{$searchQuery}%"); })
                  ->orWhereHas('kecamatan', function($sub) use ($searchQuery) { $sub->where('n_kec', 'LIKE', "%{$searchQuery}%"); })
                  ->orWhereHas('kegiatan', function($sub) use ($searchQuery) { 
                      $sub->where('nama_sub_kegiatan', 'LIKE', "%{$searchQuery}%")
                          ->orWhere('tahun_anggaran', 'LIKE', "%{$searchQuery}%");
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
            $penyedia = Penyedia::where(function ($q) use ($searchQuery) {
                $q->whereRaw("MATCH(nama, direktur) AGAINST(? IN NATURAL LANGUAGE MODE)", [$searchQuery])
                  ->orWhere('nama', 'LIKE', "%{$searchQuery}%");
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
