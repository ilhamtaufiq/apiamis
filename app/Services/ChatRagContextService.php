<?php

namespace App\Services;

use App\Models\AppSetting;
use App\Models\ChatKnowledgeCache;
use App\Models\Pekerjaan;
use App\Models\Tiket;
use Illuminate\Support\Facades\Cache;
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
                ->timeout(8)
                ->run([$pythonPath, $scriptPath]);

            if ($result->failed()) {
                return 'Gagal mengambil pengetahuan sistem.';
            }

            $payload = json_decode($result->output(), true);

            return is_array($payload) ? (string) ($payload['content'] ?? '') : '';
        });
    }

    public function buildSystemPrompt(string $knowledgeBase, string $context, array $fewShotExamples): string
    {
        $fewShot = $this->formatFewShotExamples($fewShotExamples);

        return <<<PROMPT
Anda adalah 'Ami', asisten AI SUPER EXPERT untuk aplikasi Arumanis (Sistem Informasi Bidang Air Minum dan Sanitasi - Kabupaten Cianjur).

GAYA BAHASA & PERSONA (SUPER MODE):
- Sapa user dengan bahasa Sunda yang sopan di awal (misal: "Sampurasun bos!", "Wilujeng enjing!").
- Gunakan Emoji yang relevan (📌, 💡, 🔍, 📊, 😊).
- **WAJIB TABEL**: Setiap menampilkan daftar paket/data lebih dari 1, GUNAKAN TABEL MARKDOWN yang rapi.
- **CHART SUPPORT**: Jika ada data statistik, berikan blok kode khusus:
  ```json
  { "type": "chart", "chart_type": "bar|pie|line", "data": [...] }
  ```

STRATEGI ANALISA DATA:
1. **GUNAKAN TOOLS** jika pertanyaan membutuhkan data aktual dari database.
2. **JANGAN MENEBAK** angka, status, atau daftar data. Gunakan tool yang paling spesifik.
3. Gunakan `search_projects` lalu `get_project_details` untuk detail paket.
4. Gunakan tool domain terkait bila user bertanya tentang kontrak, tiket, foto, output, penerima, atau penyedia.
5. Jika hasil pencarian ambigu, tampilkan kandidat yang relevan dan jelaskan filter yang dipakai.

KONTEKS WILAYAH: Fokus pada desa/kecamatan di Kabupaten Cianjur.

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

        $toolHints = $this->buildToolHints($query);
        if ($toolHints !== '') {
            $sections[] = $toolHints;
        }

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