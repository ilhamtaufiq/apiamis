<?php

namespace App\Services;

use App\Models\AppSetting;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Process;

class OpenRouterService
{
    protected $apiKey;
    protected $model;
    protected $baseUrl = 'https://openrouter.ai/api/v1';

    public function __construct()
    {
        $this->apiKey = config('services.openrouter.api_key', env('OPENROUTER_API_KEY'));
        
        // Priority: AppSetting > Config > Default
        $settingModel = AppSetting::getValue('openrouter_model');
        $this->model = $settingModel ?? config('services.openrouter.model', 'openrouter/free');
    }

    /**
     * Send chat completion request to OpenRouter
     */
    public function chat(array $messages, array $options = [])
    {
        $model = $options['model'] ?? $this->model;

        $payload = [
            'model' => $model,
            'messages' => $messages,
            'temperature' => $options['temperature'] ?? 0.7,
        ];

        if (isset($options['tools'])) {
            $payload['tools'] = $options['tools'];
            $payload['tool_choice'] = $options['tool_choice'] ?? 'auto';
        }

        try {
            $response = Http::withHeaders([
                'Authorization' => 'Bearer ' . $this->apiKey,
                'Content-Type' => 'application/json',
                'HTTP-Referer' => config('app.url'), // Optional, for OpenRouter ranking
                'X-Title' => config('app.name'), // Optional
            ])->timeout(120)->post($this->baseUrl . '/chat/completions', $payload);

            if ($response->failed()) {
                Log::error('OpenRouter API Error', [
                    'status' => $response->status(),
                    'body' => $response->body(),
                ]);
                return [
                    'success' => false,
                    'message' => 'AI service error: ' . ($response->json('error.message') ?? 'Unknown error'),
                ];
            }

            return [
                'success' => true,
                'content' => $response->json('choices.0.message.content'),
                'tool_calls' => $response->json('choices.0.message.tool_calls'),
                'model' => $response->json('model') ?? $model,
                'usage' => $response->json('usage'),
            ];
        } catch (\Exception $e) {
            Log::error('OpenRouter Exception', ['message' => $e->getMessage()]);
            return [
                'success' => false,
                'message' => 'Exception occurred: ' . $e->getMessage(),
            ];
        }
    }

    /**
     * Call AI using LangChain Python Bridge
     */
    public function chatWithLangChain(string $message, string $context, array $history, array $options = [])
    {
        $isWindows = PHP_OS_FAMILY === 'Windows';
        $pythonPath = $isWindows 
            ? base_path('venv/Scripts/python.exe') 
            : base_path('venv/bin/python');
        $scriptPath = base_path('scripts/chat_langchain.py');

        if (!file_exists($pythonPath)) {
            return [
                'success' => false,
                'message' => 'Python venv not found at ' . $pythonPath,
            ];
        }

        $input = [
            'api_key' => $this->apiKey,
            'model' => $options['model'] ?? $this->model,
            'message' => $message,
            'context' => $context,
            'history' => $history,
        ];

        try {
            $result = Process::input(json_encode($input))
                ->timeout(120)
                ->run([$pythonPath, $scriptPath]);

            if ($result->failed()) {
                Log::error('LangChain Script Error', [
                    'error' => $result->errorOutput(),
                    'output' => $result->output(),
                ]);
                return [
                    'success' => false,
                    'message' => 'LangChain execution failed: ' . ($result->errorOutput() ?: 'Unknown error'),
                ];
            }

            $output = json_decode($result->output(), true);

            if (!$output || !isset($output['success'])) {
                Log::error('LangChain Invalid Output', ['output' => $result->output()]);
                return [
                    'success' => false,
                    'model' => $input['model'],
                    'message' => 'Invalid output format from LangChain script',
                ];
            }

            if (!isset($output['model'])) {
                $output['model'] = $input['model'];
            }

            return $output;
        } catch (\Exception $e) {
            Log::error('LangChain Bridge Exception', ['message' => $e->getMessage()]);
            return [
                'success' => false,
                'message' => 'Bridge error: ' . $e->getMessage(),
            ];
        }
    }
}
