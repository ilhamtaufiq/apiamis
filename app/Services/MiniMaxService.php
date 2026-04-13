<?php

namespace App\Services;

use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Log;

class MiniMaxService
{
    protected $apiKey;
    protected $baseUrl = 'https://api.minimax.io/v1';

    public function __construct()
    {
        $this->apiKey = config('services.minimax.api_key', env('VITE_MINIMAX_API_KEY'));
    }

    /**
     * Send chat completion request
     */
    public function chat(array $messages, array $options = [])
    {
        $model = $options['model'] ?? 'minimax-abab6.5-chat';
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
            ])->post($this->baseUrl . '/chat/completions', $payload);

            if ($response->failed()) {
                Log::error('MiniMax API Error', [
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
            Log::error('MiniMax Exception', ['message' => $e->getMessage()]);
            return [
                'success' => false,
                'message' => 'Exception occurred: ' . $e->getMessage(),
            ];
        }
    }
}
