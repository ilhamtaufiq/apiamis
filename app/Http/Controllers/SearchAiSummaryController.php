<?php

namespace App\Http\Controllers;

use App\Services\OpenRouterService;
use Illuminate\Http\Request;
use Symfony\Component\HttpFoundation\StreamedResponse;

class SearchAiSummaryController extends Controller
{
    public function stream(Request $request, OpenRouterService $openRouter): StreamedResponse
    {
        $validated = $request->validate([
            'query' => 'required|string|max:200',
            'results' => 'required|array|max:10',
            'results.*.type' => 'nullable|string|max:50',
            'results.*.title' => 'nullable|string|max:255',
            'results.*.subtitle' => 'nullable|string|max:255',
            'results.*.penyedia' => 'nullable|string|max:255',
            'results.*.nilai' => 'nullable|numeric',
            'results.*.tahun' => 'nullable',
        ]);

        $contextText = collect($validated['results'])
            ->take(10)
            ->map(function (array $item) {
                $details = '- [' . ($item['type'] ?? 'Item') . '] ' . ($item['title'] ?? 'Tanpa judul');
                if (! empty($item['subtitle'])) {
                    $details .= ': ' . $item['subtitle'];
                }
                if (! empty($item['penyedia'])) {
                    $details .= ' (Penyedia: ' . $item['penyedia'] . ')';
                }
                if (isset($item['nilai']) && is_numeric($item['nilai'])) {
                    $details .= ' (Nilai: Rp ' . number_format((float) $item['nilai'], 0, ',', '.') . ')';
                }
                if (! empty($item['tahun'])) {
                    $details .= ' (Tahun: ' . $item['tahun'] . ')';
                }

                return $details;
            })
            ->implode("\n");

        $messages = [
            [
                'role' => 'system',
                'content' => "Anda adalah 'AmiSearch AI', asisten cerdas yang memberikan ringkasan eksekutif dan wawasan (insights) terhadap hasil pencarian di sistem Arumanis.\n\nTugas Anda:\n1. Berikan gambaran umum yang cerdas, bukan sekadar daftar ulang.\n2. Jika ada data Kontrak, sebutkan total anggaran atau tren penyedia yang dominan jika terlihat.\n3. Jika ada Progres, berikan gambaran kesehatan proyek secara keseluruhan.\n4. Gunakan gaya bahasa yang santai tapi profesional, natural, dan manusiawi.\n5. Jawab dalam Bahasa Indonesia yang fasih.\n6. Hindari format kaku. Jangan tampilkan proses berpikir logika internal.",
            ],
            [
                'role' => 'user',
                'content' => 'Berikut adalah 10 hasil pencarian teratas untuk kata kunci "' . $validated['query'] . "\":\n\n" . $contextText . "\n\nTolong berikan ringkasan yang informatif, temukan pola atau poin penting jika ada, dan sampaikan dengan cara yang menarik.",
            ],
        ];

        return response()->stream(function () use ($openRouter, $messages) {
            foreach ($openRouter->streamChatCompletion($messages) as $chunk) {
                echo 'data: ' . json_encode(['content' => $chunk], JSON_UNESCAPED_UNICODE) . "\n\n";
                if (ob_get_level() > 0) {
                    ob_flush();
                }
                flush();
            }

            echo "data: [DONE]\n\n";
            if (ob_get_level() > 0) {
                ob_flush();
            }
            flush();
        }, 200, [
            'Content-Type' => 'text/event-stream',
            'Cache-Control' => 'no-cache',
            'X-Accel-Buffering' => 'no',
        ]);
    }
}