<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use App\Services\OpenRouterService;
use App\Models\Pekerjaan;
use App\Models\Kontrak;
use App\Models\Penyedia;
use App\Models\Kegiatan;
use App\Models\Desa;
use App\Models\Tiket;
use App\Models\AuditLog;
use App\Models\SimulationNetwork;
use App\Models\DocumentRegister;
use App\Models\Event;
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
        
        // AUTO-TRIGGER: Jika frontend kirim kode ini, Ami langsung buatin laporan pagi
        if ($userMessage === '__AUTO_MORNING_REPORT__') {
            $userMessage = "Ami, sampurasun! Berikan laporan pagi atau ringkasan eksekutif hari ini.";
        }

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

        // ── 6. Call AI (via LangChain Python Bridge) ────────────
        $formattedHistory = $dbHistory->map(fn($msg) => [
            'role' => $msg->role,
            'content' => $msg->content,
        ])->toArray();

        $result = $this->openRouter->chatWithLangChain(
            $userMessage, 
            $context, 
            $formattedHistory
        );

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
        $isExecutiveSummary = str_contains($queryLower, 'laporan pagi') || str_contains($queryLower, 'ringkasan eksekutif') || str_contains($queryLower, 'summary');
        $isSearchingPhoto = str_contains($queryLower, 'foto') || str_contains($queryLower, 'gambar') || str_contains($queryLower, 'dokumentasi') || str_contains($queryLower, 'lihat');

        // 0. EXECUTIVE SUMMARY LOGIC (Proactive Reporting)
        if ($isExecutiveSummary) {
            $context .= "### EXECUTIVE CRITICAL SUMMARY (LAPORAN EKSEKUTIF):\n";
            
            // Search for stalled projects (Progress < 10% but has contract)
            $stalled = Pekerjaan::byUserRole()
                ->whereHas('kontrak')
                ->where(function($q) {
                    $q->whereDoesntHave('progress')
                      ->orWhereHas('progress', function($p) { $p->where('realisasi', '<', 10); });
                })
                ->limit(5)->get();
            
            $baseUrl = config('app.url');
            
            if ($stalled->count() > 0) {
                $context .= "- PAKET TERHAMBAT (Progres < 10%):\n";
                foreach ($stalled as $s) {
                    $url = "{$baseUrl}/pekerjaan/{$s->id}";
                    $context .= "  * [ID: {$s->id}] {$s->nama_paket} (Lokasi: " . ($s->desa->n_desa ?? 'N/A') . ") -> [Lihat Detail]({$url})\n";
                }
            }

            // Search for Open Tickets
            $openTickets = Tiket::where('status', 'open')->latest()->limit(5)->get();
            if ($openTickets->count() > 0) {
                $context .= "- TIKET MASALAH AKTIF (Belum Selesai):\n";
                foreach ($openTickets as $t) {
                    $url = $t->pekerjaan_id ? "{$baseUrl}/pekerjaan/{$t->pekerjaan_id}" : "#";
                    $context .= "  * [{$t->prioritas}] {$t->subjek} - Pelapor: " . ($t->user->name ?? 'N/A') . " -> [Buka Paket]({$url})\n";
                }
            }

            // ── NEW: Recent Activity (Last 24 Hours) ──
            $yesterday = now()->subDay();
            $recentPhotos = Foto::where('created_at', '>=', $yesterday)->with('pekerjaan')->latest()->limit(3)->get();
            $recentLogs = AuditLog::where('created_at', '>=', $yesterday)->with('user')->latest()->limit(5)->get();

            if ($recentPhotos->count() > 0 || $recentLogs->count() > 0) {
                $context .= "- AKTIVITAS TERBARU (24 Jam Terakhir):\n";
                
                // New Photos
                foreach ($recentPhotos as $f) {
                    $url = "{$baseUrl}/pekerjaan/{$f->pekerjaan_id}";
                    $context .= "  * 📸 Upload Foto: " . ($f->pekerjaan->nama_paket ?? 'Paket') . " -> [Lihat Foto]({$url})\n";
                }

                // Significant Updates from Audit Log
                foreach ($recentLogs as $log) {
                    if (in_array($log->event, ['created', 'updated'])) {
                        $modelName = class_basename($log->auditable_type);
                        if (in_array($modelName, ['Pekerjaan', 'Kontrak', 'Progress', 'Penerima', 'Tiket'])) {
                            $user = $log->user->name ?? 'User';
                            $id = $log->auditable_id;
                            $url = $modelName === 'Pekerjaan' ? "{$baseUrl}/pekerjaan/{$id}" : "#";
                            $context .= "  * ✅ {$user} melakukan {$log->event} pada data {$modelName} (ID: {$id})\n";
                        }
                    }
                }
            }
            $context .= "\n";
        }

        // Clean query from common keywords & question words for better database matching (using word boundaries)
        $stopWords = ['apa', 'bagaimana', 'siapa', 'dimana', 'kapan', 'tampilkan', 'lihat', 'cari', 'dong', 'sih', 'ya', 'kah', 'tolong', 'bisa', 'boleh', 'yang', 'di', 'ke', 'dari', 'dan', 'atau', 'berapa', 'banyak', 'jumlah', 'total', 'paket', 'pekerjaan', 'tahun', 'anggaran'];
        $regex = '/\b(' . implode('|', $stopWords) . ')\b/u';
        $cleanQuery = preg_replace($regex, '', $queryLower);
        
        // Remove non-alphanumeric except spaces
        $cleanQuery = preg_replace('/[^\w\s]/u', '', $cleanQuery);
        $cleanQuery = trim(preg_replace('/\s+/', ' ', $cleanQuery));
        
        $searchQuery = $cleanQuery;
        $year = null;
        if (preg_match('/\b(20\d{2})\b/', $query, $matches)) {
            $year = $matches[1];
            // Remove year from search query to avoid double filtering in text fields
            $searchQuery = trim(str_replace($year, '', $searchQuery));
        }

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

        $totalCount = $statsQuery->count();
        if ($totalCount > 0) {
            $allData = $statsQuery->with(['progress', 'kontrak', 'penerima'])->get();
            $avgProgress = $allData->avg(function($p) {
                return $p->progress->realisasi ?? 0;
            });
            $totalKontrak = $allData->sum(function($p) {
                return $p->kontrak->sum('nilai_kontrak');
            });
            $totalPenerima = $allData->sum(function($p) {
                return $p->penerima->count();
            });

            $context .= "### RINGKASAN STATISTIK DATA (SUMBER KEBENARAN JUMLAH):\n";
            $context .= "- TOTAL KESELURUHAN PEKERJAAN: {$totalCount} paket\n";
            $context .= "- TOTAL NILAI KONTRAK: Rp " . number_format($totalKontrak, 0, ',', '.') . "\n";
            $context .= "- TOTAL PENERIMA MANFAAT: {$totalPenerima} orang/KK\n";
            $context .= "- RATA-RATA PROGRES FISIK: " . number_format($avgProgress, 2) . "%\n";
            
            // Jika ada query tahun, berikan breakdown per sub kegiatan
            if ($year) {
                $breakdown = Pekerjaan::byUserRole()
                    ->whereHas('kegiatan', function($q) use ($year) { $q->where('tahun_anggaran', $year); })
                    ->select('kegiatan_id', DB::raw('count(*) as total'))
                    ->groupBy('kegiatan_id')
                    ->with('kegiatan')
                    ->get();
                
                if ($breakdown->count() > 0) {
                    $context .= "- RINCIAN PER KEGIATAN TA {$year}:\n";
                    foreach ($breakdown as $b) {
                        $context .= "  * " . ($b->kegiatan->nama_sub_kegiatan ?? 'Lainnya') . ": {$b->total} paket\n";
                    }
                }
            }
            $context .= "*(PENTING: Gunakan angka di atas untuk menjawab jumlah total, bukan menghitung dari daftar detail di bawah)*\n\n";
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
            ->with(['kecamatan', 'desa', 'kontrak.penyedia', 'kegiatan', 'progress', 'pengawas', 'foto', 'penerima', 'output'])
            ->where(function ($q) use ($searchQuery) {
                // Hybrid Search
                if ($searchQuery) {
                    $q->whereRaw("MATCH(nama_paket, kode_rekening) AGAINST(? IN NATURAL LANGUAGE MODE)", [$searchQuery])
                      ->orWhere('nama_paket', 'LIKE', "%{$searchQuery}%")
                      ->orWhereHas('desa', function($sub) use ($searchQuery) { $sub->where('n_desa', 'LIKE', "%{$searchQuery}%"); })
                      ->orWhereHas('kecamatan', function($sub) use ($searchQuery) { $sub->where('n_kec', 'LIKE', "%{$searchQuery}%"); })
                      ->orWhereHas('kegiatan', function($sub) use ($searchQuery) { 
                          $sub->where('nama_sub_kegiatan', 'LIKE', "%{$searchQuery}%")
                              ->orWhere('tahun_anggaran', 'LIKE', "%{$searchQuery}%");
                      });
                }
            })
            ->when($year, function($q) use ($year) {
                $q->whereHas('kegiatan', function($sub) use ($year) {
                    $sub->where('tahun_anggaran', $year);
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
                
                if ($p->kontrak->count() > 0) {
                    $context .= "  * Detail Kontrak:\n";
                    foreach ($p->kontrak as $k) {
                        $context .= "    - Nilai: Rp " . number_format($k->nilai_kontrak, 0, ',', '.') . " (" . ($k->penyedia->nama ?? 'Penyedia N/A') . ")\n";
                        $context .= "    - No. SPPBJ: " . ($k->sppbj ?? '-') . " (" . ($k->tgl_sppbj ? $k->tgl_sppbj->format('d/m/Y') : '-') . ")\n";
                        $context .= "    - No. SPK: " . ($k->spk ?? '-') . " (" . ($k->tgl_spk ? $k->tgl_spk->format('d/m/Y') : '-') . ")\n";
                        $context .= "    - No. SPMK: " . ($k->spmk ?? '-') . " (" . ($k->tgl_spmk ? $k->tgl_spmk->format('d/m/Y') : '-') . ")\n";
                        $context .= "    - Target Selesai: " . ($k->tgl_selesai ? $k->tgl_selesai->format('d/m/Y') : '-') . "\n";
                        if ($k->kode_rup) $context .= "    - Kode RUP: {$k->kode_rup}\n";
                        
                        // New: Document Registration Tracking
                        $docs = DocumentRegister::where('kontrak_id', $k->id)->with('type')->get();
                        if ($docs->count() > 0) {
                            $context .= "    - Register Dokumen: " . $docs->map(fn($d) => ($d->type->name ?? 'Dokumen') . " No: " . ($d->nomor ?? '-'))->implode(', ') . "\n";
                        }
                    }
                }

                $context .= "  * Lokasi: {$loc}\n";
                $context .= "  * Progres Fisik: {$progres}%\n";
                $context .= "  * Penerima Manfaat: " . $p->penerima->count() . " orang/KK\n";
                
                if ($p->output->count() > 0) {
                    $context .= "  * Output: ";
                    $outputs = $p->output->map(fn($o) => "{$o->komponen} ({$o->volume} {$o->satuan})")->toArray();
                    $context .= implode(", ", $outputs) . "\n";
                }

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

        // 3. Search Tickets (Issues/Complaints) if mentioned
        if (str_contains($queryLower, 'tiket') || str_contains($queryLower, 'masalah') || str_contains($queryLower, 'komplain')) {
            $tikets = Tiket::with(['user', 'pekerjaan'])
                ->where('subjek', 'LIKE', "%{$searchQuery}%")
                ->orWhere('deskripsi', 'LIKE', "%{$searchQuery}%")
                ->latest()->limit(5)->get();

            if ($tikets->count() > 0) {
                $context .= "### DATA TIKET MASALAH / KOMPLAIN:\n";
                foreach ($tikets as $t) {
                    $context .= "- Subjek: {$t->subjek} (Status: {$t->status} | Prioritas: {$t->prioritas})\n";
                    $context .= "  * Pelapor: " . ($t->user->name ?? 'N/A') . " | Deskripsi: " . substr($t->deskripsi, 0, 100) . "...\n";
                    if ($t->pekerjaan) $context .= "  * Terkait Paket: {$t->pekerjaan->nama_paket}\n";
                }
                $context .= "\n";
            }
        }

        // 4. Search Audit Logs (History) if mentioned
        if (str_contains($queryLower, 'siapa') || str_contains($queryLower, 'kapan') || str_contains($queryLower, 'history') || str_contains($queryLower, 'log')) {
            $logs = AuditLog::with('user')->latest()->limit(5)->get();
            if ($logs->count() > 0) {
                $context .= "### RIWAYAT AKTIVITAS (LOG):\n";
                foreach ($logs as $log) {
                    $context .= "- [{$log->created_at}] {$log->user->name} melakukan {$log->event} pada {$log->auditable_type} (ID: {$log->auditable_id})\n";
                }
                $context .= "\n";
            }
        }

        // 5. Fallback: Recent relevant data
        if (strlen($context) < 100) {
            $recent = Pekerjaan::byUserRole()->with(['kontrak.penyedia', 'progress'])->latest()->limit(3)->get();
            if ($recent->count() > 0) {
                $context .= "### DATA TERBARU (Mungkin Relevan):\n";
                foreach ($recent as $r) {
                    $penyediaName = $r->kontrak->first()->penyedia->nama ?? 'Belum ada penyedia';
                    $context .= "- {$r->nama_paket} | Progres: " . ($r->progress->realisasi ?? 0) . "% | Penyedia: {$penyediaName}\n";
                }
            }
        }

        // 5. Search Simulations if mentioned
        if (str_contains($queryLower, 'simulasi') || str_contains($queryLower, 'jaringan')) {
            $sims = SimulationNetwork::where('name', 'LIKE', "%{$searchQuery}%")
                ->latest()->limit(3)->get();
            if ($sims->count() > 0) {
                $context .= "### DATA SIMULASI JARINGAN TEKNIS:\n";
                foreach ($sims as $s) {
                    $context .= "- Nama: {$s->name} (Versi: " . ($s->version ?? '1.0') . ")\n";
                    $context .= "  * Deskripsi: " . ($s->description ?? 'Tidak ada deskripsi') . "\n";
                }
            }
        }

        // 6. Search Events (Calendar/Agenda) if mentioned
        if (str_contains($queryLower, 'agenda') || str_contains($queryLower, 'jadwal') || str_contains($queryLower, 'rapat') || str_contains($queryLower, 'event')) {
            $events = Event::where('title', 'LIKE', "%{$searchQuery}%")
                ->orWhere('description', 'LIKE', "%{$searchQuery}%")
                ->where('start', '>=', now())
                ->latest()->limit(5)->get();

            if ($events->count() > 0) {
                $context .= "### AGENDA & JADWAL MENDATANG:\n";
                foreach ($events as $ev) {
                    $context .= "- {$ev->title} (" . $ev->start->format('d/m/Y H:i') . ")\n";
                    if ($ev->location) $context .= "  * Lokasi: {$ev->location}\n";
                    if ($ev->description) $context .= "  * Ket: " . substr($ev->description, 0, 100) . "...\n";
                }
            }
        }

        return $context;
    }
}
