<?php

namespace App\Services;

use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Log;

class OpenRouterService
{
    protected $apiKey;
    protected $model;
    protected $baseUrl = 'https://openrouter.ai/api/v1';

    public function __construct()
    {
        $this->apiKey = config('services.openrouter.api_key', env('OPENROUTER_API_KEY'));
        $this->model = config('services.openrouter.model', 'openrouter/free');
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
}
