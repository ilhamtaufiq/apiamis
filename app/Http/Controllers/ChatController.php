<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use App\Services\OpenRouterService;
use App\Services\ChatDataToolService;
use App\Services\ChatRagContextService;
use App\Services\PekerjaanReportPdfService;
use Symfony\Component\HttpFoundation\StreamedResponse;
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
use App\Models\AppSetting;
use App\Models\Foto;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Cache;

class ChatController extends Controller
{
    protected $openRouter;
    protected $chatDataTools;
    protected $ragContextService;
    protected $reportPdf;

    public function __construct(
        OpenRouterService $openRouter,
        ChatDataToolService $chatDataTools,
        ChatRagContextService $ragContextService,
        PekerjaanReportPdfService $reportPdf,
    ) {
        $this->openRouter = $openRouter;
        $this->chatDataTools = $chatDataTools;
        $this->ragContextService = $ragContextService;
        $this->reportPdf = $reportPdf;
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
            ->limit(500)
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
     * Vote jawaban asisten: latih (up) atau tolak (down) entri knowledge.
     */
    public function voteMessage(Request $request, $id)
    {
        $request->validate(['vote' => 'required|in:up,down']);

        $message = ChatMessage::where('role', 'assistant')->findOrFail($id);
        $session = ChatSession::where('user_id', $request->user()->id)->findOrFail($message->chat_session_id);

        $prior = ChatMessage::where('chat_session_id', $session->id)
            ->where('id', '<', $message->id)
            ->where('role', 'user')
            ->orderByDesc('id')
            ->first();

        if (!$prior) {
            return response()->json(['success' => false, 'message' => 'Pertanyaan asal tidak ditemukan.'], 404);
        }

        if ($request->input('vote') === 'up') {
            ChatKnowledgeCache::learn($prior->content, '', $message->content, (int) ($message->tokens_used ?? 0));
        } else {
            ChatKnowledgeCache::downvote($prior->content);
        }

        return response()->json(['success' => true]);
    }

    /**
     * Unduh laporan PDF yang ditautkan jawaban asisten (tool generate_pekerjaan_report).
     * PDF dibangun ulang saat unduh; akses divalidasi ulang via byUserRole sesi ini.
     */
    public function downloadReport(Request $request)
    {
        $request->validate([
            'jenis' => 'required|in:paket,rekap',
            'id' => 'nullable|integer',
            'tahun' => 'nullable|integer',
            'kecamatan' => 'nullable|string|max:128',
        ]);

        try {
            if ($request->input('jenis') === 'paket') {
                $report = $this->reportPdf->generatePaket((int) $request->input('id'));
            } else {
                $report = $this->reportPdf->generateRekap(
                    $request->filled('tahun') ? (int) $request->input('tahun') : null,
                    $request->input('kecamatan'),
                );
            }
        } catch (\RuntimeException $e) {
            return response()->json(['success' => false, 'message' => $e->getMessage()], 404);
        }

        return response($report['pdf'], 200, [
            'Content-Type' => 'application/pdf',
            'Content-Disposition' => 'attachment; filename="' . $report['filename'] . '"',
        ]);
    }

    /**
     * Get messages for a session
     */
    public function sessionMessages(Request $request, $id)    {
        $session = ChatSession::where('user_id', $request->user()->id)->findOrFail($id);

        return response()->json([
            'success' => true,
            'data' => [
                'session' => $session,
                'messages' => $session->messages()->get()->map(fn($m) => [
                    'id' => $m->id,
                    'role' => $m->role,
                    'content' => $m->content,
                    'tool_calls' => $m->tool_calls,
                    'tokens_used' => $m->tokens_used,
                    'prompt_tokens' => $m->prompt_tokens,
                    'completion_tokens' => $m->completion_tokens,
                    'cost_idr' => $m->cost_idr,
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
            'provider' => 'nullable|string|max:64',
        ]);

        $userMessage = $request->input('message');
        $isAutoReport = $userMessage === '__AUTO_MORNING_REPORT__';
        
        if ($isAutoReport) {
            $userMessage = "Ami, sampurasun! Berikan laporan pagi atau ringkasan eksekutif hari ini.";
            $cacheKey = "ami_auto_report_u" . $request->user()->id;
            $cachedResponse = Cache::get($cacheKey);
            
            if ($cachedResponse) {
                return response()->json([
                    'reply' => $cachedResponse,
                    'session_id' => $request->input('session_id'),
                    'cached' => true,
                    'source' => 'token_saver'
                ]);
            }
        }

        $sessionId = $request->input('session_id');
        $user = $request->user();

        $isNewSession = false;
        $session = null;
        if ($sessionId) {
            $session = ChatSession::where('user_id', $user->id)->find($sessionId);
        }
        if (!$session) {
            $isNewSession = true;
            $session = ChatSession::create([
                'user_id' => $user->id,
                'title' => 'Percakapan Baru',
            ]);
        }

        ChatMessage::create([
            'chat_session_id' => $session->id,
            'role' => 'user',
            'content' => $userMessage,
        ]);

        // Data dinamis (angka bisa berubah tiap saat) jangan diambil dari cache belajar.
        $isDataQuery = $this->isDataQuery($userMessage);
        $cached = $isDataQuery ? null : ChatKnowledgeCache::findSimilar($userMessage);

        if ($cached && strlen($cached->response) > 50) {
            ChatMessage::create([
                'chat_session_id' => $session->id,
                'role' => 'assistant',
                'content' => $cached->response,
                'tokens_used' => 0,
            ]);

            if ($isNewSession) {
                $session->generateTitle($userMessage);
            }

            return response()->json([
                'success' => true,
                'reply' => $cached->response,
                'session_id' => $session->id,
                'cached' => true,
                'usage' => ['total_tokens' => 0],
            ]);
        }

        // Jawaban instan: pola query jelas → tabel langsung dari DB (<1s), tanpa LLM.
        $instant = $this->tryInstantAnswer($userMessage, $user->id);
        if ($instant !== null) {
            ChatMessage::create([
                'chat_session_id' => $session->id,
                'role' => 'assistant',
                'content' => $instant['reply'],
                'tool_calls' => $instant['tool_calls'],
                'tokens_used' => 0,
            ]);

            if ($isNewSession) {
                $session->generateTitle($userMessage);
            }

            $session->touch();

            return response()->json([
                'success' => true,
                'reply' => $instant['reply'],
                'session_id' => $session->id,
                'model' => 'ami-instant',
                'cached' => false,
                'instant' => true,
                'tool_calls' => $instant['tool_calls'],
                'usage' => ['total_tokens' => 0],
                'cost_idr' => null,
            ]);
        }

        $dbHistory = $session->messages()
            ->orderByDesc('id')
            ->limit(10)
            ->get()
            ->reverse()
            ->values();

        $needsKnowledge = $this->needsKnowledge($userMessage);

        $contextCacheKey = 'chat_ctx_' . md5($userMessage) . '_' . $user->id;
        $context = $needsKnowledge
            ? Cache::remember($contextCacheKey, 300, function () use ($userMessage) {
                return $this->ragContextService->buildContext($userMessage);
            })
            : '';

        $fewShotExamples = $needsKnowledge ? $this->ragContextService->getFewShotExamples() : [];

        $formattedHistory = $dbHistory->map(fn($msg) => [
            'role' => $msg->role,
            'content' => mb_substr((string) $msg->content, 0, 2000),
        ])->toArray();

        $tools = $this->selectTools($userMessage);
        $requestedProvider = $request->input('provider')
            ?? AppSetting::getValue('chat_provider')
            ?? OpenRouterService::LOCAL_PROVIDER;

        $knowledgeBase = $needsKnowledge
            ? $this->ragContextService->retrieveKnowledge($userMessage)
            : '';
        $systemPrompt = $this->ragContextService->buildSystemPrompt($knowledgeBase, $context, $fewShotExamples);
        $messages = $this->openRouter->buildChatMessages($systemPrompt, $formattedHistory, $userMessage);

        $finalResult = null;
        $loopCount = 0;
        $maxLoops = 5;

        while ($loopCount < $maxLoops) {
            $result = $this->openRouter->chatDirect($requestedProvider, $messages, [
                'tools' => $tools,
                'tool_choice' => 'auto',
                // ponytail: 1500 potong jawaban panjang; naikkan bila ringkasan eksekutif terpotong.
                'max_tokens' => 1500,
            ]);

            if (!$result['success']) {
                $finalResult = $result;
                break;
            }

            if (empty($result['tool_calls'])) {
                $finalResult = $result;
                break;
            }

            $messages[] = [
                'role' => 'assistant',
                'content' => $result['content'] ?? '',
                'tool_calls' => $result['tool_calls'],
            ];

            foreach ($this->resolveToolResults($result['tool_calls'], $userMessage) as $tr) {
                $messages[] = $tr;
            }

            $loopCount++;
        }

        if (!$finalResult['success']) {
            return response()->json($finalResult, 500);
        }

        // Guard: loop tool habis tanpa jawaban (content null + tool_calls sisa)
        // → fallback jelas, bukan reply kosong.
        if (trim((string) ($finalResult['content'] ?? '')) === '' && !empty($finalResult['tool_calls'])) {
            $names = array_values(array_unique(array_map(
                fn($tc) => $tc['function']['name'] ?? $tc['name'] ?? 'data',
                $finalResult['tool_calls']
            )));
            $aiReply = 'Data sudah saya ambil (' . implode(', ', $names) . '), tapi ringkasannya gagal disusun. Coba ulangi pertanyaan dengan lebih spesifik — mis. sebutkan nama paket atau tahunnya.';
        } else {
            $aiReply = $finalResult['content'];
        }
        $tokensUsed = $finalResult['usage']['total_tokens'] ?? 0;

        $assistantMsg = ChatMessage::create([
            'chat_session_id' => $session->id,
            'role' => 'assistant',
            'content' => $aiReply,
            'tool_calls' => $finalResult['tool_calls'] ?? null,
            'tokens_used' => $tokensUsed,
            'prompt_tokens' => $finalResult['usage']['prompt_tokens'] ?? null,
            'completion_tokens' => $finalResult['usage']['completion_tokens'] ?? null,
            'cost_idr' => $this->estimateCostIdr($finalResult['usage'] ?? null),
        ]);

        // ponytail: learn hanya jawaban berbasis tool + cukup panjang.
        // Jawaban tanpa tool (ngobrol/ngawur) tak dilatih agar tak mencemari cache.
        if (!$isDataQuery && !empty($finalResult['tool_calls']) && mb_strlen($aiReply) > 100) {
            ChatKnowledgeCache::learn($userMessage, $context, $aiReply, $tokensUsed);
        }

        if ($isAutoReport) {
            Cache::put("ami_auto_report_u" . $user->id, $aiReply, now()->addHours(4));
        }

        if ($isNewSession) {
            $session->generateTitle($userMessage);
        }

        $session->touch();

        return response()->json([
            'success' => true,
            'reply' => $aiReply,
            'session_id' => $session->id,
            'message_id' => $assistantMsg->id,
            'model' => $finalResult['model'] ?? 'ami-ai',
            'cached' => false,
            'usage' => $finalResult['usage'] ?? null,
            'prompt_tokens' => $finalResult['usage']['prompt_tokens'] ?? null,
            'completion_tokens' => $finalResult['usage']['completion_tokens'] ?? null,
            'cost_idr' => $this->estimateCostIdr($finalResult['usage'] ?? null),
        ]);
    }

    public function chatStream(Request $request): StreamedResponse
    {
        $request->validate([
            'message' => 'required|string',
            'session_id' => 'nullable|integer',
            'history' => 'nullable|array',
            'provider' => 'nullable|string|max:64',
        ]);

        $userMessage = $request->input('message');
        $user = $request->user();

        return response()->stream(function () use ($request, $userMessage, $user) {
            $emit = function (array $payload): void {
                echo 'data: ' . json_encode($payload, JSON_UNESCAPED_UNICODE) . "\n\n";
                if (ob_get_level() > 0) {
                    ob_flush();
                }
                flush();
            };

            $sessionId = $request->input('session_id');
            $session = $sessionId
                ? ChatSession::where('user_id', $user->id)->find($sessionId)
                : null;
            $isNewSession = false;

            if (!$session) {
                $isNewSession = true;
                $session = ChatSession::create([
                    'user_id' => $user->id,
                    'title' => 'Percakapan Baru',
                ]);
            }

            ChatMessage::create([
                'chat_session_id' => $session->id,
                'role' => 'user',
                'content' => $userMessage,
            ]);

            $emit(['type' => 'meta', 'session_id' => $session->id]);
            $emit(['type' => 'status', 'message' => 'Menyiapkan jawaban...']);

            $isDataQuery = $this->isDataQuery($userMessage);
            $cached = $isDataQuery ? null : ChatKnowledgeCache::findSimilar($userMessage);
            if ($cached && strlen($cached->response) > 50) {
                foreach (preg_split('/(\s+)/u', $cached->response, -1, PREG_SPLIT_DELIM_CAPTURE) as $chunk) {
                    if ($chunk === '') {
                        continue;
                    }
                    $emit(['type' => 'token', 'content' => $chunk]);
                }

                ChatMessage::create([
                    'chat_session_id' => $session->id,
                    'role' => 'assistant',
                    'content' => $cached->response,
                    'tokens_used' => 0,
                ]);

                if ($isNewSession) {
                    $session->generateTitle($userMessage);
                }

                $emit([
                    'type' => 'done',
                    'success' => true,
                    'reply' => $cached->response,
                    'session_id' => $session->id,
                    'cached' => true,
                ]);
                return;
            }

            // Jawaban instan: tabel langsung dari DB, tanpa LLM.
            $instant = $this->tryInstantAnswer($userMessage, $user->id);
            if ($instant !== null) {
                foreach (preg_split('/(\s+)/u', $instant['reply'], -1, PREG_SPLIT_DELIM_CAPTURE) as $chunk) {
                    if ($chunk !== '') {
                        $emit(['type' => 'token', 'content' => $chunk]);
                    }
                }

                ChatMessage::create([
                    'chat_session_id' => $session->id,
                    'role' => 'assistant',
                    'content' => $instant['reply'],
                    'tool_calls' => $instant['tool_calls'],
                    'tokens_used' => 0,
                ]);

                if ($isNewSession) {
                    $session->generateTitle($userMessage);
                }

                $session->touch();

                $emit([
                    'type' => 'done',
                    'success' => true,
                    'reply' => $instant['reply'],
                    'session_id' => $session->id,
                    'model' => 'ami-instant',
                    'cached' => false,
                    'instant' => true,
                ]);
                return;
            }

            $dbHistory = $session->messages()
                ->orderByDesc('id')
                ->limit(10)
                ->get()
                ->reverse()
                ->values();

            $needsKnowledge = $this->needsKnowledge($userMessage);

            $contextCacheKey = 'chat_ctx_' . md5($userMessage) . '_' . $user->id;
            $context = $needsKnowledge
                ? Cache::remember($contextCacheKey, 300, function () use ($userMessage) {
                    return $this->ragContextService->buildContext($userMessage);
                })
                : '';

            $fewShotExamples = $needsKnowledge ? $this->ragContextService->getFewShotExamples() : [];

            $formattedHistory = $dbHistory->map(fn($msg) => [
                'role' => $msg->role,
                // ponytail: histori panjang bikin prompt gemuk → TTFT naik.
                // Naikkan ke 4000 bila model kerap lupa konteks lama.
                'content' => mb_substr((string) $msg->content, 0, 2000),
            ])->toArray();

            $tools = $this->selectTools($userMessage);
            $requestedProvider = $request->input('provider')
                ?? AppSetting::getValue('chat_provider')
                ?? OpenRouterService::LOCAL_PROVIDER;

            $knowledgeBase = $needsKnowledge
                ? $this->ragContextService->retrieveKnowledge($userMessage)
                : '';
            $systemPrompt = $this->ragContextService->buildSystemPrompt($knowledgeBase, $context, $fewShotExamples);
            $messages = $this->openRouter->buildChatMessages($systemPrompt, $formattedHistory, $userMessage);

            // Stream with tools (first pass emits tokens live)
            $streamResult = $this->openRouter->streamDirect(
                $requestedProvider,
                $messages,
                function (string $token) use ($emit): void {
                    $emit(['type' => 'token', 'content' => $token]);
                },
                ['tools' => $tools, 'tool_choice' => 'auto', 'max_tokens' => 1500]
            );

            if (!($streamResult['success'] ?? false) && empty($streamResult['tool_calls'])) {
                $emit([
                    'type' => 'error',
                    'message' => $streamResult['message'] ?? 'Streaming gagal',
                ]);
                return;
            }

            $finalResult = $streamResult;

            // Tool loop (max 5 total calls, follow-ups non-streaming)
            if (!empty($streamResult['tool_calls'])) {
                $toolNames = array_values(array_unique(array_map(
                    fn($tc) => $tc['function']['name'] ?? $tc['name'] ?? 'data',
                    $streamResult['tool_calls']
                )));
                $emit(['type' => 'status', 'message' => 'Mengambil data (' . implode(', ', $toolNames) . ')...']);

                $messages[] = [
                    'role' => 'assistant',
                    'content' => $streamResult['content'] ?? '',
                    'tool_calls' => $streamResult['tool_calls'],
                ];

                foreach ($this->resolveToolResults($streamResult['tool_calls'], $userMessage) as $tr) {
                    $messages[] = $tr;
                }

                $loopCount = 1;
                $maxLoops = 5;

                while ($loopCount < $maxLoops) {
                    $nextResult = $this->openRouter->chatDirect($requestedProvider, $messages, [
                        'tools' => $tools,
                        'tool_choice' => 'auto',
                        'max_tokens' => 1500,
                    ]);

                    if (!$nextResult['success']) {
                        break;
                    }

                    if (empty($nextResult['tool_calls'])) {
                        $finalResult = $nextResult;
                        break;
                    }

                    $messages[] = [
                        'role' => 'assistant',
                        'content' => $nextResult['content'] ?? '',
                        'tool_calls' => $nextResult['tool_calls'],
                    ];
                    foreach ($this->resolveToolResults($nextResult['tool_calls'], $userMessage) as $tr) {
                        $messages[] = $tr;
                    }
                    $loopCount++;
                }

                // Emit final content as tokens (first pass only had tool calls)
                $finalContent = (string) ($finalResult['content'] ?? '');
                foreach (preg_split('/(\s+)/u', $finalContent, -1, PREG_SPLIT_DELIM_CAPTURE) as $chunk) {
                    if ($chunk !== '') {
                        $emit(['type' => 'token', 'content' => $chunk]);
                    }
                }
            }

            if (!$finalResult || !($finalResult['success'] ?? false)) {
                $emit([
                    'type' => 'error',
                    'message' => $finalResult['message'] ?? $finalResult['error'] ?? 'Streaming gagal',
                ]);
                return;
            }

            $aiReply = trim((string) ($finalResult['content'] ?? ''));
            if ($aiReply === '' && !empty($finalResult['tool_calls'])) {
                $names = array_values(array_unique(array_map(
                    fn($tc) => $tc['function']['name'] ?? $tc['name'] ?? 'data',
                    $finalResult['tool_calls']
                )));
                $aiReply = 'Data sudah saya ambil (' . implode(', ', $names) . '), tapi ringkasannya gagal disusun. Coba ulangi pertanyaan dengan lebih spesifik — mis. sebutkan nama paket atau tahunnya.';
            }
            $tokensUsed = $finalResult['usage']['total_tokens'] ?? 0;

            $assistantMsg = ChatMessage::create([
                'chat_session_id' => $session->id,
                'role' => 'assistant',
                'content' => $aiReply,
                'tool_calls' => $finalResult['tool_calls'] ?? null,
                'tokens_used' => $tokensUsed,
                'prompt_tokens' => $finalResult['usage']['prompt_tokens'] ?? null,
                'completion_tokens' => $finalResult['usage']['completion_tokens'] ?? null,
                'cost_idr' => $this->estimateCostIdr($finalResult['usage'] ?? null),
            ]);

            if (!$isDataQuery && !empty($finalResult['tool_calls']) && mb_strlen($aiReply) > 100) {
                ChatKnowledgeCache::learn($userMessage, $context, $aiReply, $tokensUsed);
            }

            if ($isNewSession) {
                $session->generateTitle($userMessage);
            }

            $session->touch();

            $emit([
                'type' => 'done',
                'success' => true,
                'reply' => $aiReply,
                'session_id' => $session->id,
                'message_id' => $assistantMsg->id,
                'model' => $finalResult['model'] ?? 'ami-ai',
                'cached' => false,
                'usage' => $finalResult['usage'] ?? null,
                'prompt_tokens' => $finalResult['usage']['prompt_tokens'] ?? null,
                'completion_tokens' => $finalResult['usage']['completion_tokens'] ?? null,
                'cost_idr' => $this->estimateCostIdr($finalResult['usage'] ?? null),
            ]);
        }, 200, [
            'Content-Type' => 'text/event-stream',
            'Cache-Control' => 'no-cache, no-transform',
            'Connection' => 'keep-alive',
            'X-Accel-Buffering' => 'no',
        ]);
    }

    private function resolveToolResults(array $toolCalls, string $userMessage = ''): array
    {
        $currentToolResults = [];

        foreach ($toolCalls as $toolCall) {
            $name = null;
            $args = [];
            $callId = $toolCall['id'] ?? ($toolCall['tool_call_id'] ?? null);

            if (isset($toolCall['function'])) {
                $name = $toolCall['function']['name'];
                $argsRaw = $toolCall['function']['arguments'];
                $args = is_string($argsRaw) ? json_decode($argsRaw, true) : $argsRaw;
            } elseif (isset($toolCall['name'])) {
                $name = $toolCall['name'];
                $args = $toolCall['args'] ?? [];
            }

            if (!$name) {
                continue;
            }

            // Slot-filling: get_project_details & laporan paket butuh ID; bila absen, cari dulu dari pesan user.
            $needsProjectId = $name === 'get_project_details'
                || ($name === 'generate_pekerjaan_report' && ($args['jenis'] ?? 'paket') === 'paket');
            if ($needsProjectId && empty($args['id']) && $userMessage !== '') {
                $resolved = $this->resolveProjectId($userMessage);
                if ($resolved !== null) {
                    $args['id'] = $resolved;
                } else {
                    $currentToolResults[] = [
                        'tool_call_id' => $callId,
                        'role' => 'tool',
                        'name' => $name,
                        'content' => json_encode([
                            'error' => 'ID paket tidak diketahui.',
                            'hint' => 'Panggil search_projects dulu dengan kata kunci dari pertanyaan user, lalu panggil ulang get_project_details dengan ID hasilnya.',
                        ]),
                    ];
                    continue;
                }
            }

            $toolOutput = $this->executeTool($name, $args ?: []);
            if (isset($toolOutput['error']) && !isset($toolOutput['hint'])) {
                $toolOutput['hint'] = $this->toolErrorHint($name);
            }
            $currentToolResults[] = [
                'tool_call_id' => $callId,
                'role' => 'tool',
                'name' => $name,
                'content' => json_encode($toolOutput),
            ];
        }

        return $currentToolResults;
    }

    /**
     * Cari ID paket dari pesan user via search_projects (top-1).
     * Return null bila tidak ada / ambigu (biar LLM klarifikasi).
     */
    private function resolveProjectId(string $userMessage): ?int
    {
        $keyword = trim(preg_replace('/\b(tolong|tolongkan|tampilkan|tampil|lihat|detail|info|data|paket|pekerjaan|proyek|cari|cek|berapa|apa|yang|di|ke|dari|untuk|tahun|\d{4})\b/iu', ' ', $userMessage));
        $keyword = trim(preg_replace('/\s+/', ' ', $keyword));
        if (mb_strlen($keyword) < 3) {
            return null;
        }

        $result = $this->executeTool('search_projects', ['keyword' => $keyword]);
        $results = $result['results'] ?? [];
        if (count($results) !== 1) {
            return null;
        }

        return (int) $results[0]['id'];
    }

    private function toolErrorHint(string $name): string
    {
        return match ($name) {
            'get_project_details' => 'Panggil search_projects dengan kata kunci lebih umum, lalu gunakan ID yang tepat.',
            'get_contractor_info' => 'Coba search_contracts dengan sebagian nama penyedia untuk ejaan yang benar.',
            'generate_pekerjaan_report' => 'Panggil search_projects dulu untuk mendapatkan ID paket, lalu ulangi generate_pekerjaan_report dengan jenis "paket" dan ID tersebut.',
            default => 'Coba longgarkan filter (hapus tahun/kata kunci spesifik) lalu ulangi.',
        };
    }

    private function getToolsDefinition()
    {
        return $this->chatDataTools->definitions();
    }

    /**
     * Jawaban instan ala search-table: pola query jelas → render tabel langsung
     * dari tool DB tanpa LLM (<1s, 0 token). Return null bila tak cocok pola.
     */
    private function tryInstantAnswer(string $query, ?int $userId = null): ?array
    {
        $q = mb_strtolower(trim($query));

        // Lanjutan halaman: "berikutnya/lanjutan/sisanya/10 berikutnya".
        if (preg_match('/\b(berikutnya|lanjutan|sisanya|selanjutnya|lanjut|more)\b/u', $q)) {
            $next = $this->nextInstantPage($userId);
            if ($next !== null) {
                return $next;
            }
        }

        // Total/ringkasan makro: "berapa total pekerjaan [tahun X]"
        if (preg_match('/\b(berapa|total|jumlah)\b.*\b(pekerjaan|paket)\b/u', $q)) {
            $args = [];
            if (preg_match('/\b(20\d{2})\b/', $q, $m)) {
                $args['tahun'] = (int) $m[1];
            }
            $stats = $this->executeTool('get_statistics', $args);
            if (isset($stats['error'])) {
                return null;
            }
            $tahun = $stats['tahun'] ?? 'semua';
            $reply = "Total **{$stats['total_pekerjaan']}** paket"
                . " · pagu **Rp " . number_format((float) $stats['total_pagu'], 0, ',', '.') . "**"
                . " · progres rata-rata **{$stats['average_progress_percent']}%**"
                . " · tiket **{$stats['total_tiket']}** (terbuka: **{$stats['open_tiket']}**)"
                . " · tahun **{$tahun}**.";

            return [
                'reply' => $reply,
                'tool_calls' => [$this->fakeToolCall('get_statistics', $args)],
            ];
        }

        // Kondisi progres: "belum 100%" / "belum selesai" / "deviasi" / "progres rendah"
        if (preg_match('/\b(belum|dibawah|di bawah|kurang dari|deviasi|terlambat|mandek|nol persen|0%)\b/u', $q)
            && preg_match('/\b(100|100%|selesai|fisik|progres|progress|50|50%)\b/u', $q)) {
            $args = [];
            if (preg_match('/\b(20\d{2})\b/', $q, $m)) {
                $args['tahun'] = (int) $m[1];
            }
            if (str_contains($q, 'deviasi') || str_contains($q, 'terlambat')) {
                $args['kondisi'] = 'behind';
            } elseif (str_contains($q, '50') && !str_contains($q, '100')) {
                $args['kondisi'] = 'low_50';
            } elseif (preg_match('/\b(nol|mandek)\b/u', $q) || preg_match('/(?<!\d)0%/u', $q)) {
                $args['kondisi'] = 'not_started';
            } else {
                $args['kondisi'] = 'incomplete';
            }

            $result = $this->executeTool('search_projects_by_progress', $args);
            $rows = $result['results'] ?? [];
            if (isset($result['error']) || ($rows instanceof \Countable ? count($rows) === 0 : $rows === [])) {
                return null;
            }

            $rows = is_array($rows) ? $rows : $rows->toArray();
            $lines = array_map(fn($r) => '| [' . str_replace('|', '/', $r['nama_paket']) . "](/pekerjaan/{$r['id']}) | {$r['lokasi']} | " . ($r['progress_fisik'] ?? '-') . '% |', $rows);

            return $this->pagedTableReply(
                "Paket yang fisiknya belum 100%:\n\n| Paket | Lokasi | Fisik % |\n|---|---|---|",
                $lines,
                'search_projects_by_progress',
                $args,
                $userId,
            );
        }

        // KPI pengawas: "kpi pengawas [nama]" / "peringkat pengawas"
        if (preg_match('/\b(kpi|kinerja|peringkat pengawas|skor pengawas|nilai pengawas)\b/u', $q)) {
            $args = [];
            if (preg_match('/\b(20\d{2})\b/', $q, $m)) {
                $args['tahun'] = (int) $m[1];
            }
            $nama = trim((string) preg_replace('/\b(tolong|tampilkan|tampil|lihat|berapa|apa|yang|di|ke|dari|untuk|kpi|kinerja|peringkat|skor|nilai|pengawas|tahun|\d{4})\b/iu', ' ', $query));
            $nama = trim((string) preg_replace('/\s+/', ' ', $nama));
            if (mb_strlen($nama) >= 3) {
                $args['nama'] = $nama;
            }

            $result = $this->executeTool('get_pengawas_kpi', $args);
            if (isset($result['error'])) {
                return null;
            }

            if (isset($args['nama'])) {
                $s = $result['ringkasan'] ?? [];
                $reply = "KPI **{$result['nama']}** ({$result['tahun']}): skor rata-rata **" . ($s['score_per_pekerjaan'] ?? '-') . "**"
                    . " · {$s['pekerjaan_count']} paket · {$s['foto_count']} foto · {$s['output_count']} output.";
            } else {
                $rows = $result['peringkat'] ?? [];
                if ($rows === []) {
                    return null;
                }
                $lines = array_map(fn($r) => "| {$r['rank']} | {$r['nama']} | {$r['skor_rata']} | {$r['paket']} |", $rows);

                return $this->pagedTableReply(
                    "Peringkat KPI pengawas {$result['tahun']}:\n\n| # | Nama | Skor | Paket |\n|---|---|---|---|",
                    $lines,
                    'get_pengawas_kpi',
                    $args,
                    $userId,
                );
            }

            return [
                'reply' => $reply,
                'tool_calls' => [$this->fakeToolCall('get_pengawas_kpi', $args)],
            ];
        }

        // Konsolidasi: "konsolidasi [nama paket]" → search top-1 lalu grup otomatis.
        if (preg_match('/\b(konsolidasi|gabungan paket|satu kontrak)\b/u', $q)) {
            $keyword = trim((string) preg_replace('/\b(tolong|tampilkan|tampil|lihat|konsolidasi|gabungan|paket|pekerjaan|satu|kontrak|yang|di|ke|dari|untuk)\b/iu', ' ', $query));
            $keyword = trim((string) preg_replace('/\s+/', ' ', $keyword));
            $searchArgs = mb_strlen($keyword) >= 3 ? ['keyword' => $keyword] : [];
            $found = $this->executeTool('search_projects', $searchArgs);
            $foundRows = $found['results'] ?? [];
            $foundRows = is_array($foundRows) ? $foundRows : $foundRows->toArray();
            $first = $foundRows[0] ?? null;
            // ponytail: keyword murni "konsolidasi" (tanpa nama paket) → jatuh ke pencarian tag generik di bawah.
            if ($first !== null) {
                $first = is_array($first) ? $first : (array) $first;

                $result = $this->executeTool('get_konsolidasi', ['id' => $first['id']]);
                if (!isset($result['error']) && !empty($result['grup'])) {
                    $g = $result['grup'][0];
                    $gPaket = $g['paket'] ?? [];
                    $gPaket = is_array($gPaket) ? $gPaket : $gPaket->toArray();
                    $lines = array_map(fn($p) => '| [' . str_replace('|', '/', $p['nama_paket']) . "](/pekerjaan/{$p['id']}) | " . number_format((float) $p['pagu'], 0, ',', '.') . ' |', $gPaket);
                    $reply = ($result['konsolidasi'] ? 'Paket konsolidasi' : 'Paket') . " \"{$result['paket']}\""
                        . " — SPK {$g['spk']} ({$g['penyedia']}), total pagu Rp " . number_format((float) $g['total_pagu'], 0, ',', '.')
                        . ":\n\n| Paket | Pagu Rp |\n|---|---|\n" . implode("\n", $lines);

                    return [
                        'reply' => $reply,
                        'tool_calls' => [$this->fakeToolCall('get_konsolidasi', ['id' => $first['id']])],
                    ];
                }
            }
        }

        // Laporan PDF: "laporan/cetak/pdf/unduh [paket|rekap]" → tautan unduh instan
        // (PDF dibangun saat endpoint unduh dipanggil — 0 token LLM).
        if (preg_match('/\b(laporan|cetak|pdf|unduh|ekspor|export)\b/u', $q)
            && preg_match('/\b(paket|pekerjaan|proyek|rekap|rekapitulasi)\b/u', $q)) {
            // Mode rekap: banyak paket (filter tahun/kecamatan).
            if (preg_match('/\b(rekap|rekapitulasi|semua paket|daftar paket|semua pekerjaan|daftar pekerjaan)\b/u', $q)) {
                $args = [];
                if (preg_match('/\b(20\d{2})\b/', $q, $m)) {
                    $args['tahun'] = (int) $m[1];
                }
                if (preg_match('/\bkecamatan\s+([a-z\s]+?)(?=\s+tahun|\s+\d{4}|$)/iu', $query, $mKec)) {
                    $args['kecamatan'] = trim($mKec[1]);
                }

                $report = $this->executeTool('generate_pekerjaan_report', $args + ['jenis' => 'rekap']);
                if (!isset($report['error'])) {
                    $s = $report['ringkasan'] ?? [];
                    $reply = "Siap! Laporan rekap {$s['total_pekerjaan']} paket"
                        . " · pagu {$s['formatted_total_pagu']}"
                        . " · rata fisik {$s['average_progress_percent']}%"
                        . ($report['judul'] ? " — {$report['judul']}" : '') . ":\n\n"
                        . "📄 [Unduh Laporan PDF]({$report['download_url']})\n\n"
                        . 'PDF kop Disperkim dibuat saat tombol diklik.';

                    return [
                        'reply' => $reply,
                        'tool_calls' => [$this->fakeToolCall('generate_pekerjaan_report', $args + ['jenis' => 'rekap'])],
                    ];
                }
            }

            // Mode paket tunggal: keyword dari pesan → search top-1 → tautan unduh.
            $keyword = trim((string) preg_replace('/\b(tolong|buat|buatkan|generate|laporan|cetak|pdf|unduh|ekspor|export|kan|paket|pekerjaan|proyek|untuk|tentang|yang|di|ke|dari|tahun|\d{4})\b/iu', ' ', $query));
            $keyword = trim((string) preg_replace('/\s+/', ' ', $keyword));
            if (mb_strlen($keyword) >= 3) {
                $found = $this->executeTool('search_projects', ['keyword' => $keyword]);
                $foundRows = $found['results'] ?? [];
                $foundRows = is_array($foundRows) ? $foundRows : $foundRows->toArray();
                if (count($foundRows) > 1) {
                    $lines = [];
                    foreach ($foundRows as $r) {
                        $r = is_array($r) ? $r : (array) $r;
                        $lines[] = '| [' . str_replace('|', '/', $r['nama_paket']) . "](/pekerjaan/{$r['id']}) | {$r['lokasi']} | " . ($r['tahun'] ?? '-') . ' |';
                    }

                    return $this->pagedTableReply(
                        "Ada " . count($foundRows) . " paket cocok \"{$keyword}\" — sebutkan nama lengkapnya untuk laporan PDF:\n\n| Paket | Lokasi | Tahun |\n|---|---|---|",
                        $lines,
                        'search_projects',
                        ['keyword' => $keyword],
                        $userId,
                    );
                }
                $first = $foundRows[0] ?? null;
                if ($first !== null) {
                    $first = is_array($first) ? (array) $first : (array) $first;
                    $report = $this->executeTool('generate_pekerjaan_report', ['jenis' => 'paket', 'id' => (int) $first['id']]);
                    if (!isset($report['error'])) {
                        $reply = "Siap! Laporan PDF untuk **{$first['nama_paket']}**"
                            . (isset($report['ringkasan']['formatted_pagu']) ? " (pagu {$report['ringkasan']['formatted_pagu']})" : '') . ":\n\n"
                            . "📄 [Unduh Laporan PDF]({$report['download_url']})\n\n"
                            . 'Isinya: identitas paket, progres, kontrak/SPK, output, penerima manfaat, dan dokumentasi foto. PDF dibuat saat tombol diklik.';

                        return [
                            'reply' => $reply,
                            'tool_calls' => [$this->fakeToolCall('generate_pekerjaan_report', ['jenis' => 'paket', 'id' => (int) $first['id']])],
                        ];
                    }
                }
            }
        }

        // Detail relasi paket: "detail/kontrak paket X" → search top-1 → kartu
        // kontrak + berkas + foto + output + tiket (tanpa LLM).
        if (preg_match('/\b(detail|kontrak|spk|relasi|kelengkapan)\b/u', $q)
            && preg_match('/\b(paket|pekerjaan|proyek)\b/u', $q)
            && !preg_match('/\b(belum|tanpa)\b/u', $q)) {
            $keyword = trim((string) preg_replace('/\b(tolong|tampilkan|tampil|lihat|detail|info|data|kontrak|spk|relasi|kelengkapan|paket|pekerjaan|proyek|yang|di|ke|dari|untuk|tahun|\d{4})\b/iu', ' ', $query));
            $keyword = trim((string) preg_replace('/\s+/', ' ', $keyword));
            if (mb_strlen($keyword) >= 3) {
                $found = $this->executeTool('search_projects', ['keyword' => $keyword]);
                $foundRows = $found['results'] ?? [];
                $foundRows = is_array($foundRows) ? $foundRows : $foundRows->toArray();
                // Multi-match → daftar pilih, bukan top-1 diam-diam.
                if (count($foundRows) > 1) {
                    $lines = [];
                    foreach ($foundRows as $r) {
                        $r = is_array($r) ? $r : (array) $r;
                        $lines[] = '| [' . str_replace('|', '/', $r['nama_paket']) . "](/pekerjaan/{$r['id']}) | {$r['lokasi']} | " . ($r['tahun'] ?? '-') . ' |';
                    }

                    return $this->pagedTableReply(
                        "Ada " . count($foundRows) . " paket cocok \"{$keyword}\" — pilih salah satu, lalu balas \"detail kontrak paket [nama lengkap]\" untuk kartu relasinya:\n\n| Paket | Lokasi | Tahun |\n|---|---|---|",
                        $lines,
                        'search_projects',
                        ['keyword' => $keyword],
                        $userId,
                    );
                }
                $first = $foundRows[0] ?? null;
                if ($first !== null) {
                    $first = is_array($first) ? (array) $first : (array) $first;
                    $instant = $this->instantProjectCard((int) $first['id']);
                    if ($instant !== null) {
                        return $instant;
                    }
                }
            }
        }

        // Detail kontrak-sentris: "kontrak/SPK/penyedia X" tanpa kata paket
        // → search_contracts top-1 → kartu paket pemilik kontrak.
        if (preg_match('/\b(kontrak|spk|penyedia|kontraktor)\b/u', $q)
            && !preg_match('/\b(paket|pekerjaan|proyek|belum|tanpa)\b/u', $q)) {
            $keyword = trim((string) preg_replace('/\b(tolong|tampilkan|tampil|lihat|detail|info|data|kontrak|spk|penyedia|kontraktor|pt|cv|yang|di|ke|dari|untuk|tahun|\d{4})\b/iu', ' ', $query));
            $keyword = trim((string) preg_replace('/\s+/', ' ', $keyword));
            if (mb_strlen($keyword) >= 2) {
                $found = $this->executeTool('search_contracts', ['keyword' => $keyword]);
                $foundRows = $found['results'] ?? [];
                $foundRows = is_array($foundRows) ? $foundRows : $foundRows->toArray();
                $first = $foundRows[0] ?? null;
                if ($first !== null && isset($first['paket_id'])) {
                    $instant = $this->instantProjectCard((int) $first['paket_id']);
                    if ($instant !== null) {
                        return $instant;
                    }
                }
            }
        }

        // Galeri lanjutan: "foto/berkas/output lainnya/lanjutan/lebih banyak [paket]"
        // → search top-1 → halaman media berikutnya (tanpa LLM).
        if (preg_match('/\b(lainnya|lanjutan|lebih banyak|selanjutnya|berikutnya|halaman)\b/u', $q)
            && preg_match('/\b(foto|berkas|dokumen|output|gambar)\b/u', $q)) {
            $jenis = str_contains($q, 'berkas') || str_contains($q, 'dokumen') ? 'berkas'
                : (str_contains($q, 'output') ? 'output' : 'foto');
            $halaman = 2;
            if (preg_match('/\bhalaman\s+(\d+)\b/u', $q, $m)) {
                $halaman = max(1, (int) $m[1]);
            }
            $keyword = trim((string) preg_replace('/\b(tolong|tampilkan|tampil|lihat|lainnya|lanjutan|lebih|banyak|selanjutnya|berikutnya|halaman|\d+|foto|berkas|dokumen|output|gambar|paket|pekerjaan|proyek|yang|di|ke|dari|untuk)\b/iu', ' ', $query));
            $keyword = trim((string) preg_replace('/\s+/', ' ', $keyword));
            if (mb_strlen($keyword) >= 3) {
                $found = $this->executeTool('search_projects', ['keyword' => $keyword]);
                $foundRows = $found['results'] ?? [];
                $foundRows = is_array($foundRows) ? $foundRows : $foundRows->toArray();
                $first = $foundRows[0] ?? null;
                if ($first !== null) {
                    $first = is_array($first) ? (array) $first : (array) $first;
                    $instant = $this->instantProjectMedia((int) $first['id'], $jenis, $halaman);
                    if ($instant !== null) {
                        return $instant;
                    }
                }
            }
        }

        // Anomali: "anomali/janggal/bermasalah/mencurigakan/tidak wajar paket".
        if (preg_match('/\b(anomali|anomali-|janggal|mencurigakan|tidak wajar|bermasalah|menyimpang)\b/u', $q)
            && preg_match('/\b(paket|pekerjaan|proyek)\b/u', $q)) {
            $args = ['limit' => 25];
            if (preg_match('/\b(20\d{2})\b/', $q, $m)) {
                $args['tahun'] = (int) $m[1];
            }
            foreach (['terlambat' => ['terlambat', 'telat', 'deviasi'], 'mandek' => ['mandek', 'macet', 'nol'], 'keuangan' => ['keuangan', 'bayar', 'cair'], 'kontrak' => ['kontrak', 'spk'], 'foto' => ['foto', 'dokumentasi']] as $jenis => $syns) {
                foreach ($syns as $syn) {
                    if (str_contains($q, $syn)) {
                        $args['jenis'] = $jenis;
                        break 2;
                    }
                }
            }
            $result = $this->executeTool('get_anomalies', $args);
            $rows = $result['results'] ?? [];
            $rows = is_array($rows) ? $rows : $rows->toArray();
            if (isset($result['error']) || $rows === []) {
                return null;
            }
            $lines = array_map(fn($r) => '| [' . str_replace('|', '/', $r['nama_paket']) . "](/pekerjaan/{$r['id']}) | " . implode(', ', $r['anomali'] ?? []) . ' | ' . str_replace('|', '/', (string) ($r['keterangan'] ?? '-')) . ' |', $rows);
            $total = $result['total'] ?? count($rows);

            return $this->pagedTableReply(
                "Anomali paket ({$result['tahun']}) — {$total} temuan:\n\n| Paket | Jenis | Keterangan |\n|---|---|---|",
                $lines,
                'get_anomalies',
                $args,
                $userId,
            );
        }

        // Paket batal: "paket batal/dibatalkan" → search_projects canceled.
        if (preg_match('/\b(batal|dibatalkan|pembatalan|cancel)\b/u', $q)
            && preg_match('/\b(paket|pekerjaan|proyek)\b/u', $q)) {
            $args = ['status' => 'canceled', 'limit' => 25];
            if (preg_match('/\b(20\d{2})\b/', $q, $m)) {
                $args['tahun'] = (int) $m[1];
            }
            $result = $this->executeTool('search_projects', $args);
            $rows = $result['results'] ?? [];
            if (isset($result['error']) || ($rows instanceof \Countable ? count($rows) === 0 : $rows === [])) {
                return null;
            }
            $rows = is_array($rows) ? $rows : $rows->toArray();
            $lines = array_map(fn($r) => '| [' . str_replace('|', '/', $r['nama_paket']) . "](/pekerjaan/{$r['id']}) | {$r['lokasi']} | " . number_format((float) $r['pagu'], 0, ',', '.') . ' | ' . str_replace('|', '/', (string) ($r['catatan'] ?? '-')) . ' |', $rows);

            return $this->pagedTableReply(
                "Paket yang statusnya batal:\n\n| Paket | Lokasi | Pagu Rp | Keterangan |\n|---|---|---|---|",
                $lines,
                'search_projects',
                $args,
                $userId,
            );
        }

        // Belum berkontrak: "belum berkontrak/belum ada kontrak/belum SPK".
        if (preg_match('/\b(belum|tanpa)\b/u', $q)
            && preg_match('/\b(berkontrak|dikontrak|kontrak|spk)\b/u', $q)) {
            $args = ['has_contract' => false];
            if (preg_match('/\b(20\d{2})\b/', $q, $m)) {
                $args['tahun'] = (int) $m[1];
            }
            $result = $this->executeTool('search_projects', $args);
            $rows = $result['results'] ?? [];
            if (isset($result['error']) || ($rows instanceof \Countable ? count($rows) === 0 : $rows === [])) {
                return null;
            }
            $rows = is_array($rows) ? $rows : $rows->toArray();
            $lines = array_map(fn($r) => '| [' . str_replace('|', '/', $r['nama_paket']) . "](/pekerjaan/{$r['id']}) | {$r['lokasi']} | " . number_format((float) $r['pagu'], 0, ',', '.') . ' |', $rows);

            return $this->pagedTableReply(
                "Paket yang belum berkontrak:\n\n| Paket | Lokasi | Pagu Rp |\n|---|---|---|",
                $lines,
                'search_projects',
                $args,
                $userId,
            );
        }

        // Ringkas wilayah: "per kecamatan/sebaran wilayah/cakupan".
        if (preg_match('/\b(per kecamatan|sebaran|cakupan|tiap kecamatan|semua kecamatan)\b/u', $q)) {
            $args = [];
            if (preg_match('/\b(20\d{2})\b/', $q, $m)) {
                $args['tahun'] = (int) $m[1];
            }
            $result = $this->executeTool('get_wilayah_summary', $args);
            $rows = $result['results'] ?? [];
            if (isset($result['error']) || ($rows instanceof \Countable ? count($rows) === 0 : $rows === [])) {
                return null;
            }
            $rows = is_array($rows) ? $rows : $rows->toArray();
            $lines = array_map(fn($r) => "| {$r['kecamatan']} | {$r['total_paket']} | " . number_format((float) $r['total_pagu'], 0, ',', '.') . " | {$r['rata_progres']}% |", $rows);

            return $this->pagedTableReply(
                "Sebaran paket per kecamatan ({$result['tahun']}):\n\n| Kecamatan | Paket | Pagu Rp | Rata Fisik |\n|---|---|---|---|",
                $lines,
                'get_wilayah_summary',
                $args,
                $userId,
            );
        }

        // Info pengawas: "siapa pengawas/pendamping [nama]" → profil + paket ditangani.
        if (preg_match('/\b(pengawas|pendamping)\b/u', $q)
            && !preg_match('/\b(kpi|kinerja|peringkat|skor|nilai)\b/u', $q)) {
            $nama = trim((string) preg_replace('/\b(tolong|tampilkan|tampil|lihat|siapa|berapa|apa|yang|di|ke|dari|untuk|oleh|daftar|list|pengawas|pendamping|nama|paket|ditangani|tangani|pekerjaan|proyek|tahun|\d{4})\b/iu', ' ', $query));
            $nama = trim((string) preg_replace('/\s+/', ' ', $nama));
            if (mb_strlen($nama) >= 3) {
                $result = $this->executeTool('get_pengawas_info', ['nama' => $nama]);
                if (!isset($result['error'])) {
                    $paketList = $result['paket_ditangani'] ?? [];
                    $paketList = $paketList instanceof \Countable && !is_array($paketList) ? $paketList->toArray() : $paketList;
                    $lines = array_map(fn($p) => '| [' . str_replace('|', '/', $p['nama_paket']) . "](/pekerjaan/{$p['id']}) | {$p['peran']} | " . ($p['tahun'] ?? '-') . ' |', $paketList);
                    $header = "**{$result['nama']}**" . ($result['jabatan'] ? " ({$result['jabatan']})" : '')
                        . ($result['telepon'] ? " · {$result['telepon']}" : '')
                        . " menangani " . count($result['paket_ditangani'] ?? []) . " paket:\n\n| Paket | Peran | Tahun |\n|---|---|---|";

                    return $this->pagedTableReply($header, $lines, 'get_pengawas_info', ['nama' => $nama], $userId);
                }
            }
        }

        // Cari generik: petakan kata kunci → tool search, render tabel instan.
        $searchMap = [
            'search_contracts' => ['kontrak', 'spk', 'penyedia', 'kontraktor', 'pt ', 'cv '],
            'search_tickets' => ['tiket', 'keluhan', 'laporan', 'masalah', 'komplain'],
            'search_photos' => ['foto', 'dokumentasi', 'gambar'],
            'search_outputs' => ['output', 'komponen', 'volume', 'terpasang', 'pipa', 'reservoir'],
            'search_recipients' => ['penerima', 'jiwa', 'kk', 'manfaat', 'beneficiary'],
            'search_kegiatan' => ['kegiatan', 'program', 'sub kegiatan', 'dpa', 'sumber dana', 'pptk'],
            'search_addendums' => ['addendum', 'perubahan kontrak', 'perpanjangan waktu'],
            'search_usulan' => ['usulan', 'surat masuk', 'permohonan', 'proposal'],
            'search_spm_sanitasi' => ['sanitasi', 'ipal', 'septik', 'truk tinja', 'mck'],
            'search_events' => ['agenda', 'jadwal', 'kalender', 'event'],
            'search_berkas' => ['berkas', 'dokumen', 'arsip', 'file'],
            'search_by_tags' => ['tag', 'label'],
            'search_projects' => ['paket', 'pekerjaan', 'proyek', 'cari', 'cek', 'lihat', 'tampilkan'],
        ];

        $toolName = null;
        foreach ($searchMap as $name => $keywords) {
            foreach ($keywords as $kw) {
                if (str_contains($q, $kw)) {
                    $toolName = $name;
                    break 2;
                }
            }
        }

        // Nama paket mentah (tanpa kata perintah/tanya): seluruh query = keyword
        // → 1 match = kartu relasi, >1 = daftar pilih, 0 = lanjut alur normal.
        // ponytail: cek kata perintah, bukan $toolName — "MCK" cocok keyword
        // search_spm_sanitasi sehingga $toolName tak pernah null untuk nama paket MCK.
        $hasCommandWord = preg_match('/\b(cari|cek|lihat|tampil|daftar|list|detail|info|berapa|total|jumlah|siapa|apa|bagaimana|kapan|mengapa|kenapa|apakah|tolong|mohon)\b/u', $q);
        if (!$hasCommandWord && mb_strlen(trim($query)) >= 10) {
            $found = $this->executeTool('search_projects', ['keyword' => trim($query)]);
            $foundRows = $found['results'] ?? [];
            $foundRows = is_array($foundRows) ? $foundRows : $foundRows->toArray();
            if (count($foundRows) === 1) {
                $first = is_array($foundRows[0]) ? $foundRows[0] : (array) $foundRows[0];
                $instant = $this->instantProjectCard((int) $first['id']);
                if ($instant !== null) {
                    return $instant;
                }
            } elseif (count($foundRows) > 1) {
                $lines = [];
                foreach ($foundRows as $r) {
                    $r = is_array($r) ? $r : (array) $r;
                    $lines[] = '| [' . str_replace('|', '/', $r['nama_paket']) . "](/pekerjaan/{$r['id']}) | {$r['lokasi']} | " . ($r['tahun'] ?? '-') . ' |';
                }

                return $this->pagedTableReply(
                    "Ada " . count($foundRows) . " paket cocok — pilih salah satu:\n\n| Paket | Lokasi | Tahun |\n|---|---|---|",
                    $lines,
                    'search_projects',
                    ['keyword' => trim($query)],
                    $userId,
                );
            }
        }

        // Hanya instan bila ada kata kerja cari ATAU tool spesifik terdeteksi
        // (mis. "kontrak maju", "agenda minggu ini").
        if ($toolName !== null || preg_match('/\b(cari|cek|lihat|tampilkan|daftar|list)\b/u', $q) || str_contains($q, 'spk')) {
            $toolName ??= 'search_projects';

            $stopwords = 'tolong|tampilkan|tampil|lihat|detail|info|data|cari|cek|berapa|apa|yang|di|ke|dari|untuk|nomor|spk|paket|pekerjaan|proyek|tahun|daftar|list|tiket|foto|output|berkas|kontrak|kegiatan|usulan|agenda|sanitasi|penerima|dokumen|komponen|volume|addendum|jadwal|kalender|event|minggu|ini|kontraktor|penyedia|tag|label';
            $keyword = trim((string) preg_replace('/\b(' . $stopwords . '|\d{4})\b/iu', ' ', $query));
            $keyword = trim((string) preg_replace('/\s+/', ' ', $keyword));

            $args = [];
            if (mb_strlen($keyword) >= 3) {
                $args['keyword'] = $keyword;
            }
            if (preg_match('/\b(20\d{2})\b/', $q, $m)) {
                $args['tahun'] = (int) $m[1];
            }
            // ponytail: status tiket hanya open/pending/closed; tambah sinonim bila user pakai istilah lain.
            if ($toolName === 'search_tickets') {
                foreach (['open' => ['terbuka', 'buka'], 'pending' => ['menunggu', 'tunda'], 'closed' => ['selesai', 'tutup', 'closed']] as $status => $syns) {
                    foreach ($syns as $syn) {
                        if (str_contains($q, $syn)) {
                            $args['status'] = $status;
                            break 2;
                        }
                    }
                }
            }

            // search_by_tags tanpa argumen tag → daftar tag (bukan results).
            if ($toolName === 'search_by_tags' && empty($args)) {
                $result = $this->executeTool($toolName, []);
                $tags = $result['tags'] ?? [];
                $tags = is_array($tags) ? $tags : $tags->toArray();
                if (isset($result['error']) || $tags === []) {
                    return null;
                }
                $lines = array_map(fn($t) => "| {$t['nama']} | {$t['jumlah_paket']} |", $tags);

                return $this->pagedTableReply(
                    "Daftar tag paket:\n\n| Tag | Paket |\n|---|---|",
                    $lines,
                    $toolName,
                    [],
                    $userId,
                );
            }

            // search_by_tags dengan kata kunci → argumen 'tag', bukan 'keyword'.
            if ($toolName === 'search_by_tags' && isset($args['keyword'])) {
                $args = ['tag' => $args['keyword']] + $args;
                unset($args['keyword']);
            }

            $result = $this->executeTool($toolName, $args);
            $rows = $result['results'] ?? [];
            if (isset($result['error']) || ($rows instanceof \Countable ? count($rows) === 0 : $rows === [])) {
                return null;
            }

            $table = $this->renderInstantTable($toolName, $keyword, is_array($rows) ? $rows : $rows->toArray());
            if ($table === null) {
                return null;
            }

            return $this->pagedTableReply($table[0], $table[1], $toolName, $args, $userId);
        }

        return null;
    }

    /**
     * Blok berkas kartu instant: ≤10 jenis → inline; >10 → tabel
     * Jenis|Jumlah + tautan unduh 5 sampel pertama.
     */
    private function berkasBlock(array $d, array $berkasSampel): string
    {
        $jenis = $d['berkas'] ?? [];
        if ($jenis === []) {
            return "Berkas: -\n";
        }
        $total = $d['berkas_count'] ?? count($jenis);
        $head = "Berkas ({$total} file, " . count($jenis) . " jenis)";
        if (count($jenis) <= 10) {
            $out = $head . ': ' . implode(', ', $jenis) . "\n";
        } else {
            $perJenis = $d['berkas_per_jenis'] ?? [];
            $rows = [];
            foreach ($jenis as $j) {
                $n = $perJenis[$j] ?? null;
                $rows[] = '| ' . str_replace('|', '/', (string) $j) . ' | ' . ($n ?? '-') . ' |';
            }
            $out = $head . ":\n\n| Jenis | File |\n|---|---|\n" . implode("\n", $rows) . "\n";
        }
        if ($berkasSampel !== []) {
            $out .= implode(' · ', array_slice($berkasSampel, 0, 5)) . "\n";
        }

        return $out;
    }

    /**
     * Kartu relasi satu paket: kontrak + addendum + dokumen + berkas +
     * foto + output + tiket (tanpa LLM). Return null bila paket tak ada.
     */
    private function instantProjectCard(int $projectId): ?array
    {
        $d = $this->executeTool('get_project_details', ['id' => $projectId]);
        if (!is_array($d) || isset($d['error'])) {
            return null;
        }

        // Tool mengembalikan Collection Eloquent; jadikan array polos dulu.
        $toArr = static fn($v) => $v instanceof \Traversable ? iterator_to_array($v, false) : (array) $v;
        foreach (['kontrak', 'addendums', 'dokumen_kontrak', 'berkas', 'berkas_sampel', 'foto_sampel', 'output_sampel', 'tiket'] as $key) {
            if (isset($d[$key]) && !is_array($d[$key])) {
                $d[$key] = $toArr($d[$key]);
            }
        }

        $kontrakLines = [];
        foreach (($d['kontrak'] ?? []) as $k) {
            $k = is_array($k) ? $k : (array) $k;
            $tgl = $k['tgl_selesai'] ?? '-';
            $kontrakLines[] = "- SPK {$k['spk']} · {$k['penyedia']} · Rp "
                . number_format((float) ($k['nilai_berjalan'] ?? $k['nilai'] ?? 0), 0, ',', '.')
                . " · selesai {$tgl}";
        }
        $addLines = [];
        foreach (($d['addendums'] ?? []) as $a) {
            $a = is_array($a) ? $a : (array) $a;
            $addLines[] = "- {$a['nomor']} ({$a['jenis']}) · Rp "
                . number_format((float) ($a['nilai_sesudah'] ?? 0), 0, ',', '.')
                . " · {$a['status']}";
        }
        $dokLines = [];
        foreach (($d['dokumen_kontrak'] ?? []) as $r) {
            $r = is_array($r) ? $r : (array) $r;
            $dokLines[] = '| ' . str_replace('|', '/', (string) ($r['jenis'] ?? '-'))
                . ' | ' . str_replace('|', '/', (string) ($r['nomor'] ?? '-'))
                . ' | ' . ($r['tanggal'] ?? '-') . ' |';
        }
        $berkasSampel = [];
        foreach (($d['berkas_sampel'] ?? []) as $b) {
            $b = is_array($b) ? $b : (array) $b;
            $label = $b['jenis_dokumen'] ?? 'dokumen';
            $berkasSampel[] = !empty($b['file_url']) ? "[{$label}]({$b['file_url']})" : $label;
        }
        $fotoLines = [];
        foreach (($d['foto_sampel'] ?? []) as $f) {
            $f = is_array($f) ? $f : (array) $f;
            $cap = $f['keterangan'] ?: 'foto';
            $fotoLines[] = !empty($f['foto_url']) ? "![{$cap}]({$f['foto_url']})" : "- {$cap}";
        }
        $outputLines = [];
        foreach (($d['output_sampel'] ?? []) as $o) {
            $o = is_array($o) ? $o : (array) $o;
            $outputLines[] = "- {$o['komponen']} · {$o['volume']} {$o['satuan']}";
        }
        $tiketOpen = count(array_filter($d['tiket'] ?? [], fn($t) => (($t['status'] ?? '') === 'open')));

        $reply = "**[{$d['nama']}](/pekerjaan/{$d['id']})** ({$d['tahun']}) · {$d['lokasi']}\n"
            . "Pagu Rp " . number_format((float) $d['pagu'], 0, ',', '.')
            . " · fisik " . ($d['progress_fisik'] ?? '-') . "% · keuangan " . ($d['progress_keuangan'] ?? '-') . "%\n\n"
            . "Kontrak (" . count($d['kontrak'] ?? []) . "):\n" . ($kontrakLines === [] ? '- belum ada' : implode("\n", $kontrakLines)) . "\n\n"
            . ($addLines === [] ? '' : "Addendum (" . count($addLines) . "):\n" . implode("\n", $addLines) . "\n\n")
            . ($dokLines === [] ? '' : "Register dokumen ([kelola](/pekerjaan/register)):\n\n| Jenis | Nomor | Tanggal |\n|---|---|---|\n" . implode("\n", $dokLines) . "\n\n")
            . $this->berkasBlock($d, $berkasSampel)
            . "Foto: {$d['jumlah_foto']}"
            . ($fotoLines === [] ? '' : "\n" . implode("\n", array_slice($fotoLines, 0, 3))) . "\n"
            . "Output: {$d['jumlah_output']}"
            . ($outputLines === [] ? '' : "\n" . implode("\n", $outputLines)) . "\n"
            . "Penerima: {$d['jumlah_penerima']} ({$d['jumlah_jiwa']} jiwa)\n"
            . "Tiket: " . count($d['tiket'] ?? []) . " (terbuka: {$tiketOpen})\n"
            . "Pengawas: " . ($d['pengawas'] ?? '-') . " · Pendamping: " . ($d['pendamping'] ?? '-');

        return [
            'reply' => $reply,
            'tool_calls' => [$this->fakeToolCall('get_project_details', ['id' => $projectId])],
        ];
    }

    /**
     * Halaman galeri lanjutan satu paket (foto/berkas/output, tanpa LLM).
     * Return null bila kosong / paket tak ada.
     */
    private function instantProjectMedia(int $projectId, string $jenis, int $halaman): ?array
    {
        $r = $this->executeTool('get_project_media', ['id' => $projectId, 'jenis' => $jenis, 'halaman' => $halaman]);
        if (!is_array($r) || isset($r['error'])) {
            return null;
        }
        $items = isset($r['items']) && !is_array($r['items'])
            ? iterator_to_array($r['items'], false)
            : ($r['items'] ?? []);
        if ($items === []) {
            return null;
        }

        $lines = [];
        foreach ($items as $it) {
            $it = is_array($it) ? $it : (array) $it;
            if ($jenis === 'foto') {
                $cap = $it['keterangan'] ?: 'foto';
                $lines[] = !empty($it['foto_url']) ? "![{$cap}]({$it['foto_url']})" : "- {$cap}";
            } elseif ($jenis === 'berkas') {
                $label = $it['jenis_dokumen'] ?? 'dokumen';
                $lines[] = !empty($it['file_url']) ? "- [{$label}]({$it['file_url']})" : "- {$label}";
            } else {
                $lines[] = "- {$it['komponen']} · {$it['volume']} {$it['satuan']}";
            }
        }

        $label = ['foto' => 'Foto', 'berkas' => 'Berkas', 'output' => 'Output'][$jenis] ?? $jenis;
        $reply = "{$label} **[{$r['paket']}](/pekerjaan/{$r['paket_id']})** — halaman {$r['halaman']}:\n\n"
            . implode("\n", $lines)
            . ($r['ada_lanjut'] ? "\n\nKetik \"{$jenis} lainnya {$r['paket']}\" untuk halaman berikutnya." : '');

        return [
            'reply' => $reply,
            'tool_calls' => [$this->fakeToolCall('get_project_media', ['id' => $projectId, 'jenis' => $jenis, 'halaman' => $halaman])],
        ];
    }

    /**
     * Render tabel markdown generik dari hasil tool search.
     * Kolom = 3 field string/angka pertama tiap baris (id disembunyikan).
     */
    /** @return array{0: string, 1: string[]}|null [header, baris] */
    private function renderInstantTable(string $toolName, string $keyword, array $rows): ?array
    {
        if ($rows === []) {
            return null;
        }

        $first = $rows[0];
        $cols = [];
        foreach ($first as $key => $value) {
            if ($key === 'id' || is_array($value) || is_bool($value)) {
                continue;
            }
            $cols[] = $key;
            if (count($cols) === 3) {
                break;
            }
        }
        if ($cols === []) {
            return null;
        }

        $label = $toolName === 'search_projects' ? "Hasil pencarian \"{$keyword}\"" : 'Hasil ' . str_replace('_', ' ', $toolName) . ($keyword !== '' ? " \"{$keyword}\"" : '');
        $header = '| ' . implode(' | ', array_map(fn($c) => ucwords(str_replace('_', ' ', (string) $c)), $cols)) . ' |';
        // ponytail: link hanya untuk baris ber-id paket; kolom foto/file URL dirender LLM sebagai gambar/link.
        $lines = array_map(function ($r) use ($cols) {
            $cells = [];
            foreach ($cols as $c) {
                $v = $r[$c] ?? '-';
                if ($v === null || $v === '') {
                    $v = '-';
                }
                if (is_float($v) && $v > 1000) {
                    $v = number_format($v, 0, ',', '.');
                }
                $v = str_replace(["\n", '|'], [' ', '/'], (string) $v);
                if (($c === 'nama_paket' || $c === 'paket') && isset($r['id']) && $v !== '-') {
                    $v = "[{$v}](/pekerjaan/{$r['id']})";
                }
                $cells[] = $v;
            }

            return '| ' . implode(' | ', $cells) . ' |';
        }, $rows);

        return [$label . ":\n\n" . $header . "\n|" . str_repeat('---|', count($cols)), $lines];
    }

    private function fakeToolCall(string $name, array $args): array
    {
        return [
            'id' => 'instant-' . $name,
            'type' => 'function',
            'function' => ['name' => $name, 'arguments' => json_encode($args, JSON_UNESCAPED_UNICODE)],
        ];
    }

    /**
     * Potong baris tabel + footer sisa ("Menampilkan X dari Y — balas
     * 'berikutnya' untuk sisanya"). Simpan sisa baris di cache per user.
     * ponytail: cache 10 menit; halaman selalu 10 baris agar chat ringkas.
     */
    private function pagedTableReply(string $header, array $lines, string $toolName, array $args, ?int $userId, int $perPage = 10): array
    {
        $total = count($lines);
        $shown = array_slice($lines, 0, $perPage);
        $rest = array_slice($lines, $perPage);
        $reply = $header . "\n" . implode("\n", $shown);
        if ($rest !== [] && $userId !== null) {
            Cache::put("ami_instant_next_u{$userId}", [
                'header' => $header,
                'lines' => $rest,
                'tool' => $toolName,
                'args' => $args,
            ], now()->addMinutes(10));
            $reply .= "\n\nMenampilkan " . count($shown) . " dari {$total} — balas **\"berikutnya\"** untuk sisanya.";
        }

        return [
            'reply' => $reply,
            'tool_calls' => [$this->fakeToolCall($toolName, $args)],
        ];
    }

    /** Halaman lanjutan dari cache pagedTableReply. */
    private function nextInstantPage(?int $userId): ?array
    {
        if ($userId === null) {
            return null;
        }
        $cached = Cache::get("ami_instant_next_u{$userId}");
        if (!is_array($cached) || empty($cached['lines'])) {
            return null;
        }
        $shown = array_slice($cached['lines'], 0, 10);
        $rest = array_slice($cached['lines'], 10);
        if ($rest === []) {
            Cache::forget("ami_instant_next_u{$userId}");
        } else {
            $cached['lines'] = $rest;
            Cache::put("ami_instant_next_u{$userId}", $cached, now()->addMinutes(10));
        }
        $reply = $cached['header'] . "\n" . implode("\n", $shown);
        if ($rest !== []) {
            $reply .= "\n\nMasih ada " . count($rest) . " lagi — balas **\"berikutnya\"** untuk lanjut.";
        }

        return [
            'reply' => $reply,
            'tool_calls' => [$this->fakeToolCall($cached['tool'] ?? 'search_projects', $cached['args'] ?? [])],
        ];
    }

    /**
     * Kirim hanya tool relevan (19 definisi = ~8.7KB per request).
     * Selalu sertakan search_projects/get_project_details sebagai jangkar paket.
     */
    private function selectTools(string $query): array
    {
        $all = $this->getToolsDefinition();
        $byName = [];
        foreach ($all as $tool) {
            $byName[$tool['function']['name'] ?? ''] = $tool;
        }

        $q = mb_strtolower($query);
        $groups = [
            'laporan' => ['generate_pekerjaan_report'],
            'pdf' => ['generate_pekerjaan_report'],
            'cetak' => ['generate_pekerjaan_report'],
            'unduh' => ['generate_pekerjaan_report'],
            'kegiatan' => ['search_kegiatan'],
            'tren' => ['get_progress_trend'],
            'addendum' => ['search_addendums'],
            'kecamatan' => ['get_wilayah_summary'],
            'wilayah' => ['get_wilayah_summary'],
            'sebaran' => ['get_wilayah_summary'],
            'usulan' => ['search_usulan'],
            'surat masuk' => ['search_usulan'],
            'sanitasi' => ['search_spm_sanitasi'],
            'ipal' => ['search_spm_sanitasi'],
            'septik' => ['search_spm_sanitasi'],
            'agenda' => ['search_events'],
            'jadwal' => ['search_events'],
            'kalender' => ['search_events'],
            'konsolidasi' => ['get_konsolidasi'],
            'gabungan paket' => ['get_konsolidasi'],
            'satu kontrak' => ['get_konsolidasi'],
            'tag' => ['search_by_tags'],
            'label' => ['search_by_tags'],
            'pengawas' => ['get_pengawas_info', 'get_pengawas_kpi'],
            'pendamping' => ['get_pengawas_info'],
            'kpi' => ['get_pengawas_kpi'],
            'kinerja' => ['get_pengawas_kpi'],
            'skor pengawas' => ['get_pengawas_kpi'],
            'nilai pengawas' => ['get_pengawas_kpi'],
            'peringkat pengawas' => ['get_pengawas_kpi'],
            'berkas' => ['search_berkas'],
            'dokumen' => ['search_berkas'],
            'arsip' => ['search_berkas'],
            'kontrak' => ['search_contracts', 'get_contractor_info'],
            'spk' => ['search_contracts'],
            'belum berkontrak' => ['search_projects'],
            'belum ada kontrak' => ['search_projects'],
            'belum dikontrak' => ['search_projects'],
            'penyedia' => ['search_contracts', 'get_contractor_info'],
            'tiket' => ['search_tickets', 'get_ticket_details'],
            'keluhan' => ['search_tickets'],
            'foto' => ['search_photos'],
            'dokumentasi' => ['search_photos'],
            'output' => ['search_outputs'],
            'volume' => ['search_outputs'],
            'komponen' => ['search_outputs'],
            'penerima' => ['search_recipients'],
            'jiwa' => ['search_recipients'],
            'manfaat' => ['search_recipients'],
            'belum 100' => ['search_projects_by_progress'],
            'belum selesai' => ['search_projects_by_progress'],
            'deviasi' => ['search_projects_by_progress'],
            'progres rendah' => ['search_projects_by_progress'],
            'progress rendah' => ['search_projects_by_progress'],
            'anomali' => ['get_anomalies'],
            'janggal' => ['get_anomalies'],
            'mencurigakan' => ['get_anomalies'],
            'bermasalah' => ['get_anomalies'],
            'tidak wajar' => ['get_anomalies'],
            'statistik' => ['get_statistics'],
            'total' => ['get_statistics'],
            'berapa' => ['get_statistics'],
            'ringkasan' => ['get_statistics'],
        ];

        $picked = ['search_projects', 'get_project_details'];
        foreach ($groups as $keyword => $names) {
            if (str_contains($q, $keyword)) {
                foreach ($names as $name) {
                    $picked[] = $name;
                }
            }
        }

        $picked = array_values(array_unique($picked));
        $subset = [];
        foreach ($picked as $name) {
            if (isset($byName[$name])) {
                $subset[] = $byName[$name];
            }
        }

        return $subset !== [] ? $subset : $all;
    }

    /**
     * Estimasi biaya Rp dari usage gateway + tarif setting.
     * Return null bila tarif belum diset (frontend sembunyikan harga).
     */
    private function estimateCostIdr(?array $usage): ?float
    {
        if ($usage === null) {
            return null;
        }

        $inputRate = (float) (AppSetting::getValue('chat_price_input_per_1m_idr') ?? 0);
        $outputRate = (float) (AppSetting::getValue('chat_price_output_per_1m_idr') ?? 0);
        if ($inputRate <= 0 && $outputRate <= 0) {
            return null;
        }

        $prompt = (int) ($usage['prompt_tokens'] ?? 0);
        $completion = (int) ($usage['completion_tokens'] ?? 0);
        if ($prompt === 0 && $completion === 0 && isset($usage['total_tokens'])) {
            $prompt = (int) $usage['total_tokens'];
        }

        return round($prompt / 1000000 * $inputRate + $completion / 1000000 * $outputRate, 2);
    }

    /** Sapaan/obrolan ringan — lewati RAG python (~4s) + prefetch + few-shot. */
    private function needsKnowledge(string $query): bool
    {
        $text = mb_strtolower(trim($query));
        if ($text === '' || mb_strlen($text) < 30) {
            $greetings = [
                'halo', 'hai', 'pagi', 'siang', 'sore', 'malam', 'sampurasun',
                'assalamu', 'terima kasih', 'makasih', 'oke', 'ok', 'siap',
                'hello', 'hi ', 'test', 'tes', 'coba',
            ];
            foreach ($greetings as $g) {
                if (str_contains($text, $g)) {
                    return false;
                }
            }
        }

        return $this->isDataQuery($text)
            || mb_strlen($text) >= 30
            || str_contains($text, '?');
    }

    /** Query data dinamis (angka/fakta DB) — jangan di-cache, selalu hitung fresh. */
    private function isDataQuery(string $query): bool
    {
        $query = mb_strtolower($query);
        $keywords = [
            'berapa', 'total', 'jumlah', 'data', 'pekerjaan', 'paket', 'kontrak', 'spk',
            'penyedia', 'progress', 'progres', 'tiket', 'foto', 'output', 'penerima',
            'kecamatan', 'desa', 'tahun', 'hari ini', 'terbaru', 'statistik', 'ringkasan',
            'cek', 'cari', 'lihat', 'tampilkan', 'status', 'detail', 'info', 'laporan',
        ];

        foreach ($keywords as $keyword) {
            if (str_contains($query, $keyword)) {
                return true;
            }
        }

        return false;
    }

    private function executeTool($name, $args)
    {
        try {
            return $this->chatDataTools->execute($name, $args);
        } catch (\Exception $e) {
            return ['error' => $e->getMessage()];
        }
    }

}


