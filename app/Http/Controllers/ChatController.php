<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use App\Services\OpenRouterService;
use App\Services\ChatDataToolService;
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
    protected $chatDataTools;

    public function __construct(OpenRouterService $openRouter, ChatDataToolService $chatDataTools)
    {
        $this->openRouter = $openRouter;
        $this->chatDataTools = $chatDataTools;
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

        $dbHistory = $session->messages()
            ->orderByDesc('id')
            ->limit(20)
            ->get()
            ->reverse()
            ->values();

        $contextCacheKey = 'chat_ctx_' . md5($userMessage) . '_' . $user->id;
        $context = Cache::remember($contextCacheKey, 300, function () use ($userMessage) {
            return $this->getDatabaseContext($userMessage);
        });

        $formattedHistory = $dbHistory->map(fn($msg) => [
            'role' => $msg->role,
            'content' => $msg->content,
        ])->toArray();

        $tools = $this->getToolsDefinition();
        $loopCount = 0;
        $maxLoops = 3;
        $toolHistory = []; 
        $requestedProvider = $request->input('provider', 'auto');

        $finalResult = null;

        while ($loopCount < $maxLoops) {
            $result = $this->openRouter->chatWithLangChain(
                $userMessage, 
                $context, 
                $formattedHistory,
                [
                    'provider' => $requestedProvider,
                    'tools' => $tools,
                    'tool_history' => $toolHistory,
                ]
            );

            if (!$result['success']) {
                $finalResult = $result;
                break;
            }

            $finalResult = $result;

            if (empty($result['tool_calls'])) {
                break;
            }

            $currentToolResults = [];
            foreach ($result['tool_calls'] as $toolCall) {
                // Determine format (OpenAI vs LangChain native)
                $name = null;
                $args = [];
                $callId = $toolCall['id'] ?? ($toolCall['tool_call_id'] ?? null);

                if (isset($toolCall['function'])) {
                    // OpenAI Format
                    $name = $toolCall['function']['name'];
                    $argsRaw = $toolCall['function']['arguments'];
                    $args = is_string($argsRaw) ? json_decode($argsRaw, true) : $argsRaw;
                } elseif (isset($toolCall['name'])) {
                    // LangChain/Direct Format
                    $name = $toolCall['name'];
                    $args = $toolCall['args'] ?? [];
                }

                if (!$name) continue;
                
                $toolOutput = $this->executeTool($name, $args ?: []);
                $currentToolResults[] = [
                    'tool_call_id' => $callId,
                    'role' => 'tool',
                    'name' => $name,
                    'content' => json_encode($toolOutput),
                ];
            }

            $toolHistory[] = [
                'assistant' => [
                    'role' => 'assistant',
                    'content' => $result['content'] ?? '',
                    'tool_calls' => $result['tool_calls']
                ],
                'results' => $currentToolResults
            ];
            
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

        if ($session->messages()->count() <= 2) {
            $session->generateTitle();
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

    private function getDatabaseContext($query)
    {
        $context = "";
        $queryLower = strtolower($query);
        $isExecutiveSummary = str_contains($queryLower, 'laporan pagi') || str_contains($queryLower, 'ringkasan eksekutif');
        
        if ($isExecutiveSummary) {
            $context .= "### EXECUTIVE SUMMARY (REAL-TIME):\n";
            $totalPagu = Pekerjaan::byUserRole()->sum('pagu');
            $context .= "- Total Pagu Terkelola: Rp " . number_format($totalPagu, 0, ',', '.') . "\n";
            $context .= "- Tiket Open: " . Tiket::where('status', 'open')->count() . "\n\n";
        }

        return $context;
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


