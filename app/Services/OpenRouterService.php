<?php

namespace App\Services;

use App\Models\AppSetting;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Process;

class OpenRouterService
{
    private const PROVIDERS = [
        'openrouter' => [
            'label' => 'OpenRouter',
            'base_url' => 'https://openrouter.ai/api/v1',
            'api_key_env' => 'OPENROUTER_API_KEY',
            'default_model' => 'openai/gpt-oss-120b:free',
        ],
        'openai' => [
            'label' => 'OpenAI',
            'base_url' => 'https://api.openai.com/v1',
            'api_key_env' => 'OPENAI_API_KEY',
            'default_model' => 'gpt-4o-mini',
        ],
        'gemini' => [
            'label' => 'Google Gemini',
            'base_url' => 'https://generativelanguage.googleapis.com/v1beta/openai',
            'api_key_env' => 'GEMINI_API_KEY',
            'default_model' => 'gemini-2.5-flash-lite',
        ],
        'deepseek' => [
            'label' => 'DeepSeek',
            'base_url' => 'https://api.deepseek.com/v1',
            'api_key_env' => 'DEEPSEEK_API_KEY',
            'default_model' => 'deepseek-chat',
        ],
        'groq' => [
            'label' => 'Groq',
            'base_url' => 'https://api.groq.com/openai/v1',
            'api_key_env' => 'GROQ_API_KEY',
            'default_model' => 'llama-3.3-70b-versatile',
        ],
        'mistral' => [
            'label' => 'Mistral AI',
            'base_url' => 'https://api.mistral.ai/v1',
            'api_key_env' => 'MISTRAL_API_KEY',
            'default_model' => 'mistral-small-latest',
        ],
        'github-models' => [
            'label' => 'GitHub Models',
            'base_url' => 'https://models.github.ai/inference',
            'api_key_env' => 'GITHUB_TOKEN',
            'default_model' => 'openai/gpt-4.1',
            'headers' => [
                'Accept' => 'application/vnd.github+json',
                'X-GitHub-Api-Version' => '2022-11-28',
            ],
        ],
        'huggingface' => [
            'label' => 'Hugging Face',
            'base_url' => 'https://router.huggingface.co/v1',
            'api_key_env' => 'HF_TOKEN',
            'default_model' => 'meta-llama/Meta-Llama-3.1-8B-Instruct:fastest',
        ],
        'nebius' => [
            'label' => 'Nebius',
            'base_url' => 'https://api.studio.nebius.com/v1',
            'api_key_env' => 'NEBIUS_API_KEY',
            'default_model' => 'gpt-oss-120b',
        ],
        'nscale' => [
            'label' => 'Nscale',
            'base_url' => 'https://inference.api.nscale.com/v1',
            'api_key_env' => 'NSCALE_API_KEY',
            'default_model' => 'gpt-oss-120b',
        ],
        'cerebras' => [
            'label' => 'Cerebras',
            'base_url' => 'https://api.cerebras.ai/v1',
            'api_key_env' => 'CEREBRAS_API_KEY',
            'default_model' => 'gpt-oss-120b',
        ],
        'xai' => [
            'label' => 'xAI',
            'base_url' => 'https://api.x.ai/v1',
            'api_key_env' => 'XAI_API_KEY',
            'default_model' => 'grok-4.1-fast',
        ],
        'ai21' => [
            'label' => 'AI21 Labs',
            'base_url' => 'https://api.ai21.com/studio/v1',
            'api_key_env' => 'AI21_API_KEY',
            'default_model' => 'jamba-mini-2',
        ],
        'aionlabs' => [
            'label' => 'Aion Labs',
            'base_url' => 'https://api.aionlabs.ai/v1',
            'api_key_env' => 'AIONLABS_API_KEY',
            'default_model' => 'aion-2.0',
        ],
        'alibaba' => [
            'label' => 'Alibaba Cloud Model Studio',
            'base_url' => 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1',
            'api_key_env' => 'DASHSCOPE_API_KEY',
            'default_model' => 'qwen3-plus',
        ],
        'zai' => [
            'label' => 'Z AI',
            'base_url' => 'https://open.bigmodel.cn/api/paas/v4',
            'api_key_env' => 'ZAI_API_KEY',
            'default_model' => 'glm-4.7-flash',
        ],
        'kilo' => [
            'label' => 'Kilo Code',
            'base_url' => 'https://api.kilo.ai/api/gateway',
            'api_key_env' => 'KILO_API_KEY',
            'default_model' => 'kilo-auto/free',
        ],
        'llm7' => [
            'label' => 'LLM7.io',
            'base_url' => 'https://api.llm7.io/v1',
            'api_key_env' => 'LLM7_API_KEY',
            'default_model' => 'gpt-4o-mini',
        ],
        'modelscope' => [
            'label' => 'ModelScope',
            'base_url' => 'https://api-inference.modelscope.cn/v1',
            'api_key_env' => 'MODELSCOPE_API_KEY',
            'default_model' => 'Qwen/Qwen3.5-27B',
        ],
        'nvidia' => [
            'label' => 'NVIDIA NIM',
            'base_url' => 'https://integrate.api.nvidia.com/v1',
            'api_key_env' => 'NVIDIA_API_KEY',
            'default_model' => 'deepseek-ai/deepseek-r1',
        ],
        'siliconflow' => [
            'label' => 'SiliconFlow',
            'base_url' => 'https://api.siliconflow.cn/v1',
            'api_key_env' => 'SILICONFLOW_API_KEY',
            'default_model' => 'Qwen/Qwen3-8B',
        ],
        'ovhcloud' => [
            'label' => 'OVHcloud AI Endpoints',
            'base_url' => 'https://oai.endpoints.kepler.ai.cloud.ovh.net/v1',
            'api_key_env' => null,
            'default_model' => 'Meta-Llama-3_3-70B-Instruct',
        ],
    ];

    private const UNSUPPORTED_PROVIDERS = ['cohere', 'cloudflare-workers-ai', 'ollama'];

    protected $apiKey;
    protected $model;
    protected $fallbackModel;
    protected $provider;
    protected $baseUrl;

    public function __construct()
    {
        $this->provider = AppSetting::getValue('chat_provider')
            ?? AppSetting::getValue('openrouter_provider')
            ?? 'auto';

        $providerConfig = $this->getProviderConfig($this->provider);

        $this->baseUrl = AppSetting::getValue('chat_base_url') ?: ($providerConfig['base_url'] ?? 'https://openrouter.ai/api/v1');
        $this->apiKey = $this->resolveApiKey($this->provider, $providerConfig);

        $this->model = $providerConfig['default_model'] ?? config('services.openrouter.model', 'openai/gpt-oss-120b:free');
        $this->fallbackModel = config('services.openrouter.fallback_model', 'z-ai/glm-4.5-air:free');
    }

    public static function providerOptions(): array
    {
        return self::PROVIDERS;
    }

    private function providerSettingKey(string $provider): string
    {
        return 'chat_api_key_' . str_replace('-', '_', $provider);
    }

    private function getProviderConfig(string $provider): array
    {
        if ($provider === 'auto') {
            return self::PROVIDERS['openrouter'];
        }

        if (in_array($provider, self::UNSUPPORTED_PROVIDERS, true)) {
            return [];
        }

        return self::PROVIDERS[$provider] ?? self::PROVIDERS['openrouter'];
    }

    private function resolveApiKey(string $provider, array $providerConfig): ?string
    {
        $settingValue = AppSetting::getValue($this->providerSettingKey($provider));
        if (!empty($settingValue)) {
            return $settingValue;
        }

        $envKey = $providerConfig['api_key_env'] ?? 'OPENROUTER_API_KEY';
        if (!$envKey) {
            return null;
        }

        $apiKey = env($envKey);
        if ($apiKey) {
            return $apiKey;
        }

        if ($envKey === 'OPENROUTER_API_KEY') {
            return config('services.openrouter.api_key', env('OPENROUTER_API_KEY'));
        }

        return null;
    }

    private function resolveHeaders(array $providerConfig, ?string $apiKey = null): array
    {
        $headers = $providerConfig['headers'] ?? [];

        if (($providerConfig['label'] ?? null) === 'GitHub Models' && !empty($apiKey)) {
            $headers['Authorization'] = 'Bearer ' . $apiKey;
        }

        return $headers;
    }

    private function getAutoRotationProviders(): array
    {
        $eligible = [];

        foreach (array_keys(self::PROVIDERS) as $provider) {
            if (in_array($provider, self::UNSUPPORTED_PROVIDERS, true)) {
                continue;
            }

            $providerConfig = $this->getProviderConfig($provider);
            $apiKey = $this->resolveApiKey($provider, $providerConfig);

            if (($providerConfig['api_key_env'] ?? null) !== null && empty($apiKey)) {
                continue;
            }

            $eligible[] = $provider;
        }

        return $eligible;
    }

    private function getRotationStartIndex(int $providerCount): int
    {
        if ($providerCount <= 0) {
            return 0;
        }

        return ((int) Cache::get('ami_chat_provider_rotation_index', 0)) % $providerCount;
    }

    private function advanceRotationIndex(int $nextIndex, int $providerCount): void
    {
        if ($providerCount <= 0) {
            return;
        }

        Cache::put('ami_chat_provider_rotation_index', $nextIndex % $providerCount, now()->addDays(7));
    }

    private function runHttpAttempt(string $provider, array $messages, array $options = []): array
    {
        $providerConfig = $this->getProviderConfig($provider);

        if (in_array($provider, self::UNSUPPORTED_PROVIDERS, true)) {
            return [
                'success' => false,
                'message' => 'Provider AI "' . $provider . '" belum didukung oleh bridge chat saat ini.',
            ];
        }

        $model = $options['model'] ?? ($providerConfig['default_model'] ?? $this->model);
        $baseUrl = $options['base_url'] ?? ($providerConfig['base_url'] ?? $this->baseUrl);
        $apiKey = $options['api_key'] ?? $this->resolveApiKey($provider, $providerConfig);
        $headers = array_merge(
            $this->resolveHeaders($providerConfig, $apiKey),
            $options['headers'] ?? []
        );

        if (($providerConfig['api_key_env'] ?? null) !== null && empty($apiKey)) {
            return [
                'success' => false,
                'message' => 'API key untuk provider "' . $provider . '" belum diset.',
            ];
        }

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
            $requestHeaders = array_merge(['Content-Type' => 'application/json'], $headers);

            if (!empty($apiKey)) {
                $requestHeaders['Authorization'] = 'Bearer ' . $apiKey;
            }

            $response = Http::withHeaders($requestHeaders)
                ->timeout(120)
                ->post(rtrim($baseUrl, '/') . '/chat/completions', $payload);

            if ($response->failed()) {
                Log::error('OpenRouter API Error', [
                    'provider' => $provider,
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
            Log::error('OpenRouter Exception', ['provider' => $provider, 'message' => $e->getMessage()]);

            return [
                'success' => false,
                'message' => 'Exception occurred: ' . $e->getMessage(),
            ];
        }
    }

    private function runLangChainAttempt(string $provider, string $message, string $context, array $history, array $options = []): array
    {
        $providerConfig = $this->getProviderConfig($provider);

        if (in_array($provider, self::UNSUPPORTED_PROVIDERS, true)) {
            return [
                'success' => false,
                'message' => 'Provider AI "' . $provider . '" belum didukung oleh bridge chat saat ini.',
            ];
        }

        $apiKey = $options['api_key'] ?? $this->resolveApiKey($provider, $providerConfig);
        $baseUrl = $options['base_url'] ?? ($providerConfig['base_url'] ?? $this->baseUrl);
        $headers = array_merge(
            $this->resolveHeaders($providerConfig, $apiKey),
            $options['headers'] ?? []
        );

        if (($providerConfig['api_key_env'] ?? null) !== null && empty($apiKey)) {
            return [
                'success' => false,
                'message' => 'API key untuk provider "' . $provider . '" belum diset.',
            ];
        }

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

        $model = $options['model'] ?? ($providerConfig['default_model'] ?? $this->model);

        $input = [
            'api_key' => $apiKey,
            'provider' => $provider,
            'model' => $model,
            'base_url' => $baseUrl,
            'headers' => $headers,
            'message' => $message,
            'context' => $context,
            'history' => $history,
            'tools' => $options['tools'] ?? null,
            'tool_history' => $options['tool_history'] ?? null,
        ];

        try {
            $result = Process::input(json_encode($input))
                ->timeout(120)
                ->run([$pythonPath, $scriptPath]);

            if ($result->failed()) {
                Log::error('LangChain Script Error', [
                    'provider' => $provider,
                    'error' => $result->errorOutput(),
                    'output' => $result->output(),
                    'model' => $input['model'],
                ]);

                if (($options['allow_model_fallback'] ?? true) && $model !== $this->fallbackModel) {
                    return $this->runLangChainAttempt($provider, $message, $context, $history, array_merge($options, [
                        'model' => $this->fallbackModel,
                        'base_url' => $baseUrl,
                        'api_key' => $apiKey,
                        'headers' => $headers,
                        'allow_model_fallback' => false,
                    ]));
                }

                return [
                    'success' => false,
                    'message' => 'LangChain execution failed: ' . ($result->errorOutput() ?: 'Unknown error'),
                ];
            }

            $output = json_decode($result->output(), true);

            if (!$output || !isset($output['success'])) {
                Log::error('LangChain Invalid Output', ['provider' => $provider, 'output' => $result->output()]);

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
            Log::error('LangChain Bridge Exception', ['provider' => $provider, 'message' => $e->getMessage()]);

            return [
                'success' => false,
                'message' => 'Bridge error: ' . $e->getMessage(),
            ];
        }
    }

    private function runWithRotation(string $requestedProvider, callable $attempt, array $options = []): array
    {
        $providersToTry = $requestedProvider === 'auto'
            ? $this->getAutoRotationProviders()
            : [$requestedProvider];

        if (empty($providersToTry)) {
            return [
                'success' => false,
                'message' => 'Tidak ada provider AI yang siap dipakai. Periksa API key di settings.',
            ];
        }

        $rotationStart = $requestedProvider === 'auto'
            ? $this->getRotationStartIndex(count($providersToTry))
            : 0;

        $orderedProviders = array_merge(
            array_slice($providersToTry, $rotationStart),
            array_slice($providersToTry, 0, $rotationStart)
        );

        $lastFailure = null;

        foreach ($orderedProviders as $index => $provider) {
            $attemptOptions = $options;

            if ($requestedProvider === 'auto') {
                unset($attemptOptions['model']);
            }

            $result = $attempt($provider, $attemptOptions);

            if (!empty($result['success'])) {
                if ($requestedProvider === 'auto') {
                    $this->advanceRotationIndex($rotationStart + $index + 1, count($providersToTry));
                }

                return $result;
            }

            $lastFailure = $result;
        }

        return $lastFailure ?: [
            'success' => false,
            'message' => 'Semua provider AI gagal merespons.',
        ];
    }

    /**
     * Send chat completion request to the configured provider set
     */
    public function chat(array $messages, array $options = [])
    {
        $requestedProvider = $options['provider'] ?? $this->provider;

        return $this->runWithRotation($requestedProvider, function (string $provider, array $attemptOptions) use ($messages) {
            return $this->runHttpAttempt($provider, $messages, $attemptOptions);
        }, $options);
    }

    /**
     * Call AI using LangChain Python Bridge
     */
    public function chatWithLangChain(string $message, string $context, array $history, array $options = [])
    {
        $requestedProvider = $options['provider'] ?? $this->provider;

        return $this->runWithRotation($requestedProvider, function (string $provider, array $attemptOptions) use ($message, $context, $history) {
            return $this->runLangChainAttempt($provider, $message, $context, $history, $attemptOptions);
        }, $options);
    }
}
