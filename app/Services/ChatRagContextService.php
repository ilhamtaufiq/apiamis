<?php

namespace App\Services;

use App\Models\AppSetting;
use App\Models\ChatKnowledgeCache;
use App\Models\Pekerjaan;
use App\Models\Tiket;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Process;

class ChatRagContextService
{
    public function __construct(
        private readonly ChatDataToolService $chatDataTools,
    ) {}

    public function retrieveKnowledge(string $query): string
    {
        $cacheKey = 'chat_rag_knowledge_' . hash('sha256', mb_strtolower(trim($query)));

        return Cache::remember($cacheKey, 600, function () use ($query) {
            $isWindows = PHP_OS_FAMILY === 'Windows';
            $pythonPath = $isWindows
                ? base_path('venv/Scripts/python.exe')
                : base_path('venv/bin/python');
            $scriptPath = base_path('scripts/rag_query.py');

            if (!file_exists($pythonPath) || !file_exists($scriptPath)) {
                return 'Pengetahuan sistem belum tersedia.';
            }

            $result = Process::input(json_encode(['query' => $query]))
                ->timeout(30)
                ->run([$pythonPath, $scriptPath]);

            if ($result->failed()) {
                Log::warning('RAG query failed', ['stderr' => $result->errorOutput()]);
                return 'Gagal mengambil pengetahuan sistem.';
            }

            $payload = json_decode($result->output(), true);
            if (isset($payload['error'])) {
                Log::warning('RAG query error', ['error' => $payload['error']]);
                return 'Pengetahuan sistem belum tersedia.';
            }

            return is_array($payload) ? (string) ($payload['content'] ?? '') : '';
        });
    }

    public function buildSystemPrompt(string $knowledgeBase, string $context, array $fewShotExamples): string
    {
        $fewShot = $this->formatFewShotExamples($fewShotExamples);
        $defaultYear = AppSetting::getValue('tahun_anggaran') ?? (int) date('Y');

        return <<<PROMPT
Anda adalah 'Ami', asisten AI untuk aplikasi Arumanis (air minum dan sanitasi Kabupaten Cianjur).
Bicara Bahasa Indonesia yang santai dan natural seperti sedang ngobrol — BUKAN seperti laporan, bukan template.
Jangan selalu membuka dengan sapaan atau emoji; sapa hanya sesekali bila cocok saja.

ATURAN DATA (WAJIB):
1. Setiap pertanyaan tentang data (paket, kontrak, penyedia, progres, tiket, foto, output, penerima, kegiatan, addendum, wilayah, usulan, sanitasi, agenda, pengawas, berkas) WAJIB dijawab dari hasil tool, bukan dari ingatan.
2. JANGAN menebak angka, nama, status, atau ID. Bila ID belum diketahui, cari dulu via tool search.
3. Tahun anggaran berjalan adalah {$defaultYear}. Bila user tidak menyebut tahun tapi maksudnya data tahun berjalan (mis. "total pekerjaan", "kontrak terbaru"), isi argumen tahun dengan {$defaultYear}.
4. Bila tool mengembalikan hasil kosong: katakan terus-terang ("Data tidak ditemukan untuk filter X"), lalu tawarkan alternatif (ubah kata kunci, tahun lain, atau tampilkan data terkait).
5. Bila hasil ambigu (banyak kandidat mirip): tampilkan maksimal 5 kandidat sebagai tabel dan minta user memilih, jangan asal pilih satu.
6. Alur detail paket: `search_projects` (dapat ID) → `get_project_details` (detail lengkap).
7. Tren/progres bulanan: `search_projects` (dapat ID) → `get_progress_trend`, jawab sertakan chart line dari `tren_bulanan`.
8. Agregat wilayah: `get_wilayah_summary` (tanpa argumen = semua kecamatan).
9. Detail tiket: `search_tickets` (dapat ID) → `get_ticket_details` (detail + komentar).
10. KPI pengawas: `get_pengawas_kpi` (nama opsional; kosong = peringkat semua).
11. Konsolidasi: `search_projects` (dapat ID) → `get_konsolidasi` (grup paket satu kontrak).
12. Tags: `search_by_tags` (tanpa argumen = daftar tag; dengan tag = paket ber-tag).

GAYA JAWABAN (natural, bukan template):
- Ceritakan dengan kalimat biasa. Tabel hanya bila datanya memang banyak dan perlu dibandingkan.
- Chart JSON hanya untuk statistik/tren yang divisualkan; JANGAN tiap jawaban dipaksa ada chart.
- Filter (tahun, kecamatan, kata kunci) sebutkan sekilas di akhir bila relevan, bukan sebagai baris "Filter dipakai:" yang kaku.
- Jangan tutup dengan penawaran bantuan yang itu-itu saja ("Ada lagi yang bisa saya bantu?").

ATURAN TAMPILAN (WAJIB):
- JANGAN pernah tampilkan ID/nomor internal (id paket, id tiket, dsb.) ke user. ID hanya untuk memanggil tool.
- Setiap nama paket tulis sebagai link: [Nama Paket](/pekerjaan/ID). Contoh: [SPAM Cibeber](/pekerjaan/581).
- Bila tool mengembalikan foto_url: tampilkan sebagai gambar markdown ![keterangan](foto_url) (maksimal 6 gambar per jawaban).
- Bila tool mengembalikan file_url: tampilkan sebagai link [jenis_dokumen](file_url).

KONTEKS WILAYAH: Kabupaten Cianjur.

CONTOH JAWABAN TERBAIK (FEW-SHOT):
{$fewShot}

PENGETAHUAN SISTEM (RETRIEVED):
{$knowledgeBase}

KONTEKS DATA AWAL (STATIC):
{$context}
PROMPT;
    }

    private function formatFewShotExamples(array $examples): string
    {
        if ($examples === []) {
            return 'Tidak ada contoh jawaban tersimpan.';
        }

        $blocks = [];
        foreach ($examples as $index => $example) {
            $query = trim((string) ($example['query'] ?? ''));
            $response = trim((string) ($example['response'] ?? ''));
            if ($query === '' || $response === '') {
                continue;
            }

            $blocks[] = 'Contoh ' . ($index + 1) . ":\nPertanyaan: {$query}\nJawaban: {$response}";
        }

        return $blocks === [] ? 'Tidak ada contoh jawaban tersimpan.' : implode("\n\n", $blocks);
    }

    public function buildContext(string $query): string
    {
        $sections = [];

        if ($this->isDataQuery($query)) {
            $prefetch = $this->buildPrefetchStats($query);
            if ($prefetch !== '') {
                $sections[] = $prefetch;
            }
        }

        if ($this->isExecutiveSummaryQuery($query)) {
            $sections[] = $this->buildExecutiveSummary();
        }

        return implode("\n\n", array_filter($sections));
    }

    public function getFewShotExamples(int $limit = 2): array
    {
        return ChatKnowledgeCache::getFewShotExamples($limit);
    }

    private function buildToolHints(string $query): string
    {
        $queryLower = mb_strtolower($query);
        $hints = [];

        $routes = [
            ['tools' => ['get_statistics'], 'keywords' => ['berapa', 'total', 'jumlah', 'statistik', 'ringkasan', 'overview']],
            ['tools' => ['search_projects', 'get_project_details'], 'keywords' => ['pekerjaan', 'paket', 'proyek', 'detail paket']],
            ['tools' => ['search_contracts', 'get_contractor_info'], 'keywords' => ['kontrak', 'spk', 'penyedia', 'kontraktor']],
            ['tools' => ['search_tickets'], 'keywords' => ['tiket', 'laporan', 'keluhan', 'issue']],
            ['tools' => ['search_photos'], 'keywords' => ['foto', 'dokumentasi', 'gambar']],
            ['tools' => ['search_outputs'], 'keywords' => ['output', 'komponen', 'volume']],
            ['tools' => ['search_recipients'], 'keywords' => ['penerima', 'jiwa', 'kk', 'beneficiary']],
        ];

        foreach ($routes as $route) {
            foreach ($route['keywords'] as $keyword) {
                if (!str_contains($queryLower, $keyword)) {
                    continue;
                }

                $hints[] = '- Pertanyaan ini berkaitan dengan data live → prioritaskan tool: `'
                    . implode('`, `', $route['tools']) . '`.';
                break;
            }
        }

        if ($hints === []) {
            return '';
        }

        return "### PETUNJUK TOOL (ROUTING):\n" . implode("\n", array_unique($hints));
    }

    private function buildPrefetchStats(string $query): string
    {
        $args = [];
        $tahun = AppSetting::getValue('tahun_anggaran');
        if ($tahun) {
            $args['tahun'] = (int) $tahun;
        }

        if (preg_match('/\b(20\d{2})\b/', $query, $matches)) {
            $args['tahun'] = (int) $matches[1];
        }

        $stats = $this->chatDataTools->execute('get_statistics', $args);
        if (isset($stats['error'])) {
            return '';
        }

        return "### SNAPSHOT DATABASE (PREFETCH):\n"
            . '- Total pekerjaan: ' . ($stats['total_pekerjaan'] ?? 0) . "\n"
            . '- Total pagu: ' . ($stats['formatted_total_pagu'] ?? '-') . "\n"
            . '- Rata-rata progres: ' . ($stats['average_progress_percent'] ?? 0) . "%\n"
            . '- Total tiket: ' . ($stats['total_tiket'] ?? 0) . ' (open: ' . ($stats['open_tiket'] ?? 0) . ")\n"
            . '- Filter tahun: ' . ($stats['tahun'] ?? 'semua');
    }

    private function buildExecutiveSummary(): string
    {
        $totalPagu = Pekerjaan::byUserRole()->sum('pagu');

        return "### EXECUTIVE SUMMARY (REAL-TIME):\n"
            . '- Total Pagu Terkelola: Rp ' . number_format($totalPagu, 0, ',', '.') . "\n"
            . '- Tiket Open: ' . Tiket::where('status', 'open')->count();
    }

    private function isExecutiveSummaryQuery(string $query): bool
    {
        $queryLower = mb_strtolower($query);

        return str_contains($queryLower, 'laporan pagi')
            || str_contains($queryLower, 'ringkasan eksekutif');
    }

    private function isDataQuery(string $query): bool
    {
        $queryLower = mb_strtolower($query);
        $keywords = [
            'berapa', 'total', 'jumlah', 'data', 'pekerjaan', 'paket', 'kontrak', 'spk',
            'penyedia', 'progress', 'progres', 'tiket', 'foto', 'output', 'penerima',
            'kecamatan', 'desa', 'tahun', 'hari ini', 'terbaru', 'statistik',
        ];

        foreach ($keywords as $keyword) {
            if (str_contains($queryLower, $keyword)) {
                return true;
            }
        }

        return false;
    }
}