<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use App\Services\MiniMaxService;
use App\Models\Pekerjaan;
use App\Models\Kontrak;
use App\Models\Penyedia;
use App\Models\Kegiatan;
use App\Models\Desa;
use Illuminate\Support\Facades\DB;

class ChatController extends Controller
{
    protected $miniMax;

    public function __construct(MiniMaxService $miniMax)
    {
        $this->miniMax = $miniMax;
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

        // 1. Get Context from Database based on user message
        $context = $this->getDatabaseContext($userMessage);

        // 2. Prepare Messages for AI
        $messages = [];
        
        // System Prompt with Schema Knowledge
        $messages[] = [
            'role' => 'system',
            'content' => "Anda adalah 'Ami', asisten AI cerdas untuk aplikasi Arumanis (Sistem Informasi Pekerjaan Umum). 
            Tugas Anda adalah membantu pengguna menjawab pertanyaan seputar data proyek (Pekerjaan), Kontrak, Penyedia (Kontraktor), dan Kegiatan.

            STRUKTUR DATA ARUMANIS:
            - Pekerjaan: Unit proyek utama (memiliki nama paket, pagu, lokasi desa/kecamatan).
            - Kegiatan: Kategori besar yang menaungi beberapa Pekerjaan.
            - Kontrak: Detail kesepakatan hukum antara Pekerjaan dan Penyedia.
            - Penyedia: Perusahaan/Kontraktor yang mengerjakan proyek.
            - Lokasi: Pekerjaan selalu berada di suatu Desa dan Kecamatan.

            SKILLS & TOOLS:
            Anda memiliki akses ke fungsi 'input_pekerjaan', 'input_kontrak', dan 'input_kegiatan'.
            1. Jika pengguna ingin menginput data, periksa apakah data yang dibutuhkan sudah lengkap sesuai parameter fungsi.
            2. Jika ada data yang kurang (misal: ID Desa atau ID Kecamatan), tanya kepada pengguna secara sopan.
            3. Jangan memanggil fungsi jika data wajib belum lengkap, kecuali Anda ingin mengonfirmasi data yang sudah ada.
            4. Selalu gunakan Nama Desa/Kecamatan asli dari database (lihat konteks di bawah) untuk mendapatkan ID yang benar.

            Berikut adalah konteks data relasional dari database yang sesuai dengan pertanyaan pengguna:\n\n" . $context . "\n\n
            PANDUAN:
            1. Jika pengguna bertanya tentang siapa yang mengerjakan proyek, lihat data Kontrak -> Penyedia.
            2. Jika pengguna bertanya tentang lokasi, lihat data Desa/Kecamatan di Pekerjaan.
            3. Gunakan data di atas untuk menjawab secara detail dan informatif.
            4. Jika data tidak ditemukan, sarankan pengguna untuk memberikan kata kunci yang lebih spesifik.
            Bahasa: Indonesia."
        ];

        // Add history
        foreach ($history as $msg) {
            $messages[] = $msg;
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
        $result = $this->miniMax->chat($messages, [
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
     * Relational RAG: Search database with eager loading
     */
    private function getDatabaseContext($query)
    {
        $context = "";
        $queryLower = strtolower($query);

        // 1. Search Pekerjaan with its relations
        $pekerjaan = Pekerjaan::with(['kecamatan', 'desa', 'kontrak.penyedia', 'kegiatan'])
            ->whereRaw("MATCH(nama_paket, kode_rekening) AGAINST(? IN BOOLEAN MODE)", [$query])
            ->orWhereHas('kecamatan', function($q) use ($query) { $q->whereRaw("MATCH(n_kec) AGAINST(? IN BOOLEAN MODE)", [$query]); })
            ->orWhereHas('desa', function($q) use ($query) { $q->whereRaw("MATCH(n_desa) AGAINST(? IN BOOLEAN MODE)", [$query]); })
            ->limit(5)->get();

        if ($pekerjaan->count() > 0) {
            $context .= "### DATA PEKERJAAN & RELASI:\n";
            foreach ($pekerjaan as $p) {
                $loc = ($p->desa->n_desa ?? '-') . ", " . ($p->kecamatan->n_kec ?? '-');
                $context .= "- Paket: [{$p->id}] {$p->nama_paket}\n";
                $context .= "  * Pagu: Rp " . number_format($p->pagu, 0, ',', '.') . "\n";
                $context .= "  * Lokasi: {$loc}\n";
                $context .= "  * Kegiatan: " . ($p->kegiatan->nama_sub_kegiatan ?? '-') . "\n";
                
                if ($p->kontrak->count() > 0) {
                    $context .= "  * Kontrak & Penyedia:\n";
                    foreach ($p->kontrak as $k) {
                        $context .= "    - No SPK: {$k->spk}, Penyedia: " . ($k->penyedia->nama ?? 'N/A') . " (Nilai: Rp " . number_format($k->nilai_kontrak, 0, ',', '.') . ")\n";
                    }
                }
                $context .= "\n";
            }
        }

        // 2. Search Penyedia/Kontraktor directly if mentioned
        if (str_contains($queryLower, 'kontraktor') || str_contains($queryLower, 'penyedia') || $pekerjaan->count() < 2) {
            $penyedia = Penyedia::with(['kontrak.pekerjaan'])
                ->whereRaw("MATCH(nama, direktur) AGAINST(? IN BOOLEAN MODE)", [$query])
                ->limit(3)->get();

            if ($penyedia->count() > 0) {
                $context .= "### DATA PENYEDIA (KONTRAKTOR):\n";
                foreach ($penyedia as $pen) {
                    $context .= "- Nama: {$pen->nama} (Direktur: {$pen->direktur})\n";
                    if ($pen->kontrak->count() > 0) {
                        $context .= "  * Mengerjakan Paket:\n";
                        foreach ($pen->kontrak as $k) {
                            $context .= "    - " . ($k->pekerjaan->nama_paket ?? 'Paket tidak ditemukan') . " (SPK: {$k->spk})\n";
                        }
                    }
                    $context .= "\n";
                }
            }
        }

        // 3. IF context still empty or very short, get summary of recent activities
        if (strlen($context) < 100) {
            $recent = Pekerjaan::with('kontrak.penyedia')->latest()->limit(3)->get();
            $context .= "### DATA TERBARU LAINNYA:\n";
            foreach ($recent as $r) {
                $penyediaName = $r->kontrak->first()->penyedia->nama ?? 'Belum ada penyedia';
                $context .= "- Paket: {$r->nama_paket} | Pagu: Rp " . number_format($r->pagu, 0, ',', '.') . " | Penyedia: {$penyediaName}\n";
            }
        }

        return $context;
    }
}
