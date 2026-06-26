<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use App\Services\OpenRouterService;
use App\Services\ChatDataToolService;
use App\Services\ChatRagContextService;
use App\Services\ChatLangChainBridge;
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
    protected $langChainBridge;

    public function __construct(
        OpenRouterService $openRouter,
        ChatDataToolService $chatDataTools,
        ChatRagContextService $ragContextService,
        ChatLangChainBridge $langChainBridge,
    ) {
        $this->openRouter = $openRouter;
        $this->chatDataTools = $chatDataTools;
        $this->ragContextService = $ragContextService;
        $this->langChainBridge = $langChainBridge;
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

        $cached = null;
        if (!$this->isDynamicDataQuery($userMessage)) {
            $cached = ChatKnowledgeCache::findSimilar($userMessage);
        }

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

        $dbHistory = $session->messages()
            ->orderByDesc('id')
            ->limit(20)
            ->get()
            ->reverse()
            ->values();

        $contextCacheKey = 'chat_ctx_' . md5($userMessage) . '_' . $user->id;
        $context = Cache::remember($contextCacheKey, 300, function () use ($userMessage) {
            return $this->ragContextService->buildContext($userMessage);
        });

        $fewShotExamples = $this->isDynamicDataQuery($userMessage)
            ? []
            : $this->ragContextService->getFewShotExamples();

        $formattedHistory = $dbHistory->map(fn($msg) => [
            'role' => $msg->role,
            'content' => $msg->content,
        ])->toArray();

        $tools = $this->getToolsDefinition();
        $loopCount = 0;
        $maxLoops = 3;
        $toolHistory = [];
        $requestedProvider = $request->input('provider')
            ?? AppSetting::getValue('chat_provider')
            ?? OpenRouterService::LOCAL_PROVIDER;

        $knowledgeBase = $this->ragContextService->retrieveKnowledge($userMessage);
        $systemPrompt = $this->ragContextService->buildSystemPrompt($knowledgeBase, $context, $fewShotExamples);
        $chatMessages = $this->openRouter->buildChatMessages($systemPrompt, $formattedHistory, $userMessage);
        $generationOptions = $this->buildGenerationOptions(
            $requestedProvider,
            $tools,
            $toolHistory,
            $fewShotExamples,
            $knowledgeBase,
            $systemPrompt,
        );

        $finalResult = null;

        if ($requestedProvider === OpenRouterService::LOCAL_PROVIDER) {
            $finalResult = $this->openRouter->chatDirect($requestedProvider, $chatMessages);
        }

        while (($finalResult === null || !($finalResult['success'] ?? false)) && $loopCount < $maxLoops) {
            $result = $this->openRouter->chatWithLangChain(
                $userMessage,
                $context,
                $formattedHistory,
                $generationOptions
            );

            if (!$result['success']) {
                $finalResult = $result;
                break;
            }

            $finalResult = $result;

            if (empty($result['tool_calls'])) {
                break;
            }

            $currentToolResults = $this->resolveToolResults($result['tool_calls']);

            $toolHistory[] = [
                'assistant' => [
                    'role' => 'assistant',
                    'content' => $result['content'] ?? '',
                    'tool_calls' => $result['tool_calls']
                ],
                'results' => $currentToolResults
            ];

            $generationOptions['tool_history'] = $toolHistory;
            $loopCount++;
        }

        if (!$finalResult['success']) {
            return response()->json($finalResult, 500);
        }

        $aiReply = $finalResult['content'];
        $tokensUsed = $finalResult['usage']['total_tokens'] ?? 0;

        ChatMessage::create([
            'chat_session_id' => $session->id,
            'role' => 'assistant',
            'content' => $aiReply,
            'tool_calls' => $finalResult['tool_calls'] ?? null,
            'tokens_used' => $tokensUsed,
        ]);

        ChatKnowledgeCache::learn($userMessage, $context, $aiReply, $tokensUsed);

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
            'model' => $finalResult['model'] ?? 'ami-ai',
            'cached' => false,
            'usage' => $finalResult['usage'] ?? null,
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

            if (!$this->isDynamicDataQuery($userMessage)) {
                $cached = ChatKnowledgeCache::findSimilar($userMessage);
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
            }

            $dbHistory = $session->messages()
                ->orderByDesc('id')
                ->limit(20)
                ->get()
                ->reverse()
                ->values();

            $contextCacheKey = 'chat_ctx_' . md5($userMessage) . '_' . $user->id;
            $context = Cache::remember($contextCacheKey, 300, function () use ($userMessage) {
                return $this->ragContextService->buildContext($userMessage);
            });

            $fewShotExamples = $this->isDynamicDataQuery($userMessage)
                ? []
                : $this->ragContextService->getFewShotExamples();

            $formattedHistory = $dbHistory->map(fn($msg) => [
                'role' => $msg->role,
                'content' => $msg->content,
            ])->toArray();

            $tools = $this->getToolsDefinition();
            $toolHistory = [];
            $requestedProvider = $request->input('provider')
                ?? AppSetting::getValue('chat_provider')
                ?? OpenRouterService::LOCAL_PROVIDER;

            $knowledgeBase = $this->ragContextService->retrieveKnowledge($userMessage);
            $systemPrompt = $this->ragContextService->buildSystemPrompt($knowledgeBase, $context, $fewShotExamples);
            $chatMessages = $this->openRouter->buildChatMessages($systemPrompt, $formattedHistory, $userMessage);
            $finalResult = null;

            if ($requestedProvider === OpenRouterService::LOCAL_PROVIDER) {
                $finalResult = $this->openRouter->streamDirect(
                    $requestedProvider,
                    $chatMessages,
                    function (string $token) use ($emit): void {
                        $emit(['type' => 'token', 'content' => $token]);
                    }
                );
            }

            if ($finalResult === null || !($finalResult['success'] ?? false)) {
                $runtime = $this->openRouter->providerRuntime($requestedProvider);
                $generationOptions = $this->buildGenerationOptions(
                    $requestedProvider,
                    $tools,
                    $toolHistory,
                    $fewShotExamples,
                    $knowledgeBase,
                    $systemPrompt,
                );

                $payload = array_merge($generationOptions, [
                    'api_key' => $runtime['api_key'],
                    'model' => $runtime['model'],
                    'base_url' => $runtime['base_url'],
                    'headers' => $runtime['headers'],
                    'message' => $userMessage,
                    'context' => $context,
                    'history' => $formattedHistory,
                ]);

                $loopCount = 0;
                $maxLoops = 3;
                $finalResult = null;

                while ($loopCount < $maxLoops) {
                    $streamResult = $this->langChainBridge->stream($payload, function (array $event) use ($emit): void {
                        if (($event['type'] ?? null) === 'token' && !empty($event['content'])) {
                            $emit(['type' => 'token', 'content' => $event['content']]);
                        }
                    });

                    $isToolCallStep = ($streamResult['type'] ?? null) === 'tool_calls'
                        || !empty($streamResult['tool_calls']);

                    if (!($streamResult['success'] ?? false) && !$isToolCallStep) {
                        $emit([
                            'type' => 'error',
                            'message' => $streamResult['message'] ?? $streamResult['error'] ?? 'Streaming gagal',
                        ]);
                        return;
                    }

                    if ($isToolCallStep) {
                        $emit(['type' => 'status', 'message' => 'Mengambil data dari database...']);
                        $currentToolResults = $this->resolveToolResults($streamResult['tool_calls']);
                        $toolHistory[] = [
                            'assistant' => [
                                'role' => 'assistant',
                                'content' => $streamResult['content'] ?? '',
                                'tool_calls' => $streamResult['tool_calls'],
                            ],
                            'results' => $currentToolResults,
                        ];

                        $payload['tool_history'] = $toolHistory;
                        $generationOptions['tool_history'] = $toolHistory;
                        $loopCount++;
                        continue;
                    }

                    $finalResult = $streamResult;
                    break;
                }
            }

            if (!$finalResult || !($finalResult['success'] ?? false)) {
                $emit([
                    'type' => 'error',
                    'message' => $finalResult['message'] ?? $finalResult['error'] ?? 'Streaming gagal',
                ]);
                return;
            }

            $aiReply = (string) ($finalResult['content'] ?? '');
            $tokensUsed = $finalResult['usage']['total_tokens'] ?? 0;

            ChatMessage::create([
                'chat_session_id' => $session->id,
                'role' => 'assistant',
                'content' => $aiReply,
                'tool_calls' => $finalResult['tool_calls'] ?? null,
                'tokens_used' => $tokensUsed,
            ]);

            ChatKnowledgeCache::learn($userMessage, $context, $aiReply, $tokensUsed);

            if ($isNewSession) {
                $session->generateTitle($userMessage);
            }

            $session->touch();

            $emit([
                'type' => 'done',
                'success' => true,
                'reply' => $aiReply,
                'session_id' => $session->id,
                'model' => $finalResult['model'] ?? 'ami-ai',
                'cached' => false,
                'usage' => $finalResult['usage'] ?? null,
            ]);
        }, 200, [
            'Content-Type' => 'text/event-stream',
            'Cache-Control' => 'no-cache, no-transform',
            'Connection' => 'keep-alive',
            'X-Accel-Buffering' => 'no',
        ]);
    }

    private function buildGenerationOptions(
        string $provider,
        array $tools,
        array $toolHistory,
        array $fewShotExamples,
        string $knowledgeBase,
        string $systemPrompt,
    ): array {
        return [
            'provider' => $provider,
            'tools' => $tools,
            'tool_history' => $toolHistory,
            'few_shot_examples' => $fewShotExamples,
            'knowledge_base' => $knowledgeBase,
            'system_prompt' => $systemPrompt,
        ];
    }

    private function resolveToolResults(array $toolCalls): array
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

            $toolOutput = $this->executeTool($name, $args ?: []);
            $currentToolResults[] = [
                'tool_call_id' => $callId,
                'role' => 'tool',
                'name' => $name,
                'content' => json_encode($toolOutput),
            ];
        }

        return $currentToolResults;
    }

    private function getToolsDefinition()
    {
        return $this->chatDataTools->definitions();
    }

    private function executeTool($name, $args)
    {
        try {
            return $this->chatDataTools->execute($name, $args);
        } catch (\Exception $e) {
            return ['error' => $e->getMessage()];
        }
    }

    private function isDynamicDataQuery(string $query): bool
    {
        $query = strtolower($query);
        $keywords = [
            'berapa', 'total', 'jumlah', 'data', 'pekerjaan', 'paket', 'kontrak', 'spk',
            'penyedia', 'progress', 'progres', 'tiket', 'foto', 'output', 'penerima',
            'kecamatan', 'desa', 'tahun', 'hari ini', 'terbaru',
        ];

        foreach ($keywords as $keyword) {
            if (str_contains($query, $keyword)) {
                return true;
            }
        }

        return false;
    }
}


