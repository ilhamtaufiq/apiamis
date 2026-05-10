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
            'content' => "Anda adalah 'Ami', asisten AI cerdas untuk aplikasi Arumanis (Sistem Informasi Pekerjaan Umum). 
            Tugas Anda adalah membantu pengguna menjawab pertanyaan seputar data proyek (Pekerjaan), Kontrak, Penyedia (Kontraktor), dan Kegiatan.

            PANDUAN VISUAL:
            1. Jika konteks data menyediakan URL foto, tampilkan foto tersebut menggunakan format Markdown: ![Keterangan](URL).
            2. Berikan informasi progres fisik secara jelas.

            STRUKTUR DATA ARUMANIS:
            ... (seperti sebelumnya) ...

            Berikut adalah konteks data relasional dari database yang sesuai dengan pertanyaan pengguna:\n\n" . $context . "\n\n
            PANDUAN JAWABAN:
            1. Jika pengguna bertanya tentang siapa yang mengerjakan proyek, lihat data Kontrak -> Penyedia.
            2. Jika pengguna bertanya tentang lokasi, lihat data Desa/Kecamatan di Pekerjaan.
            3. Gunakan data di atas untuk menjawab secara detail dan informatif.
            4. Jika ada foto, tampilkan foto tersebut untuk membantu visualisasi.
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

        // 3. Define Tools (Skills)
        $tools = [
            [
                'type' => 'function',
                'function' => [
                    'name' => 'input_pekerjaan',
                    'description' => 'Gunakan fungsi ini untuk menginput data paket pekerjaan baru ke sistem.',
                    'parameters' => [
                        'type' => 'object',
                        'properties' => [
                            'nama_paket' => ['type' => 'string', 'description' => 'Nama paket pekerjaan/proyek'],
                            'pagu' => ['type' => 'number', 'description' => 'Nilai pagu anggaran'],
                            'kecamatan_id' => ['type' => 'integer', 'description' => 'ID Kecamatan'],
                            'desa_id' => ['type' => 'integer', 'description' => 'ID Desa'],
                            'kode_rekening' => ['type' => 'string', 'description' => 'Kode rekening proyek'],
                            'kegiatan_id' => ['type' => 'integer', 'description' => 'ID Kegiatan terkait'],
                        ],
                        'required' => ['nama_paket', 'pagu', 'kecamatan_id', 'desa_id']
                    ]
                ]
            ],
            [
                'type' => 'function',
                'function' => [
                    'name' => 'input_kontrak',
                    'description' => 'Gunakan fungsi ini untuk menginput data kontrak baru untuk suatu pekerjaan.',
                    'parameters' => [
                        'type' => 'object',
                        'properties' => [
                            'id_pekerjaan' => ['type' => 'integer', 'description' => 'ID Pekerjaan terpilih'],
                            'id_penyedia' => ['type' => 'integer', 'description' => 'ID Penyedia/Kontraktor'],
                            'nilai_kontrak' => ['type' => 'number', 'description' => 'Nilai kontrak yang disepakati'],
                            'nomor_spk' => ['type' => 'string', 'description' => 'Nomor Surat Perintah Kerja (SPK)'],
                        ],
                        'required' => ['id_pekerjaan', 'id_penyedia', 'nilai_kontrak', 'nomor_spk']
                    ]
                ]
            ],
            [
                'type' => 'function',
                'function' => [
                    'name' => 'input_kegiatan',
                    'description' => 'Gunakan fungsi ini untuk menginput data program kegiatan baru.',
                    'parameters' => [
                        'type' => 'object',
                        'properties' => [
                            'nama_program' => ['type' => 'string'],
                            'nama_kegiatan' => ['type' => 'string'],
                            'nama_sub_kegiatan' => ['type' => 'string'],
                            'tahun_anggaran' => ['type' => 'string', 'description' => 'Tahun (contoh: 2024)'],
                        ],
                        'required' => ['nama_kegiatan', 'tahun_anggaran']
                    ]
                ]
            ],
            [
                'type' => 'function',
                'function' => [
                    'name' => 'get_locations',
                    'description' => 'Gunakan fungsi ini untuk mendapatkan daftar ID Kecamatan dan Desa agar input data akurat.',
                    'parameters' => [
                        'type' => 'object',
                        'properties' => [
                            'search' => ['type' => 'string', 'description' => 'Nama desa atau kecamatan'],
                        ],
                    ]
                ]
            ]
        ];

        // 4. Call AI
        $result = $this->openRouter->chat($messages, [
            'model' => 'nvidia/nemotron-3-super-120b-a12b:free',
            'tools' => $tools,
            'tool_choice' => 'auto'
        ]);

        if (!$result['success']) {
            return response()->json($result, 500);
        }

        // 5. Check for Tool Calls
        if (!empty($result['tool_calls'])) {
            return response()->json([
                'success' => true,
                'reply' => 'Sedang menyiapkan formulir input...',
                'tool_calls' => $result['tool_calls'],
                'usage' => $result['usage'] ?? null
            ]);
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

        // 1. Search Pekerjaan with its relations (Limited by user role for security)
        $pekerjaan = Pekerjaan::byUserRole()
            ->with(['kecamatan', 'desa', 'kontrak.penyedia', 'kegiatan', 'progress', 'pengawas', 'foto'])
            ->where(function ($q) use ($query) {
                // Hybrid Search: Full-Text + LIKE Fallback
                $q->whereRaw("MATCH(nama_paket, kode_rekening) AGAINST(? IN NATURAL LANGUAGE MODE)", [$query])
                  ->orWhere('nama_paket', 'LIKE', "%{$query}%");
            })
            ->limit(5)->get();

        if ($pekerjaan->count() > 0) {
            $context .= "### DATA PEKERJAAN & PROGRES:\n";
            foreach ($pekerjaan as $p) {
                $loc = ($p->desa->n_desa ?? '-') . ", " . ($p->kecamatan->n_kec ?? '-');
                $progres = $p->progress->realisasi ?? 0;
                $pengawas = $p->pengawas->nama ?? 'Belum ditunjuk';
                
                $context .= "- Paket: [ID: {$p->id}] {$p->nama_paket}\n";
                $context .= "  * Pagu: Rp " . number_format($p->pagu, 0, ',', '.') . "\n";
                $context .= "  * Lokasi: {$loc}\n";
                $context .= "  * Progres Fisik: {$progres}%\n";
                $context .= "  * Pengawas Lapangan: {$pengawas}\n";
                $context .= "  * Sub Kegiatan: " . ($p->kegiatan->nama_sub_kegiatan ?? '-') . "\n";
                
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
