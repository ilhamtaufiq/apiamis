<?php

namespace App\Services;

use App\Models\AppSetting;
use App\Models\ChatKnowledgeCache;
use App\Models\Pekerjaan;
use App\Models\Tiket;

class ChatRagContextService
{
    public function __construct(
        private readonly ChatDataToolService $chatDataTools,
    ) {}

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