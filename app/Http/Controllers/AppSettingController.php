<?php

namespace App\Http\Controllers;

use App\Models\AppSetting;
use App\Http\Resources\AppSettingResource;
use App\Services\OpenRouterService;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Str;

class AppSettingController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/settings",
     *     summary="Get all application settings",
     *     tags={"Settings"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index()
    {
        $settings = AppSetting::all();
        return AppSettingResource::collection($settings);
    }

    /**
     * @OA\Post(
     *     path="/api/settings",
     *     summary="Update application settings",
     *     description="Handles text values and file uploads (logo, favicon)",
     *     tags={"Settings"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 @OA\Property(property="app_name", type="string"),
     *                 @OA\Property(property="app_description", type="string"),
     *                 @OA\Property(property="tahun_anggaran", type="string"),
     *                 @OA\Property(property="logo", type="string", format="binary"),
     *                 @OA\Property(property="favicon", type="string", format="binary")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=200, description="Settings updated")
     * )
     */
    public function store(Request $request)
    {
        $request->validate([
            'app_name' => 'nullable|string|max:255',
            'app_description' => 'nullable|string|max:500',
            'tahun_anggaran' => 'nullable|string|max:4',
            'chat_provider' => 'nullable|string|max:64',
            'chat_base_url' => 'nullable|string|max:255',
            'chat_model' => 'nullable|string|max:128',
            'chat_api_key' => 'nullable|string|max:2000',
            'landing_page_active' => 'nullable|string|in:0,1',
            'spm_detail_page_active' => 'nullable|string|in:0,1',
            'puspen_progress_fisik_public' => 'nullable|string|in:0,1',
            'logo' => 'nullable|file|mimes:jpg,jpeg,png,svg|max:2048',
            'favicon' => 'nullable|file|mimes:jpg,jpeg,png,svg,ico|max:1024',
        ]);

        $apiKeyProviders = array_merge(array_keys(OpenRouterService::providerOptions()), ['local']);

        foreach ($apiKeyProviders as $providerId) {
            $settingKey = $this->chatApiKeySettingKey($providerId);
            $request->validate([
                $settingKey => 'nullable|string|max:2000',
            ]);
        }

        $updatedSettings = [];

        // Handle text settings
        if ($request->has('app_name')) {
            $setting = AppSetting::setValue('app_name', $request->app_name, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('app_description')) {
            $setting = AppSetting::setValue('app_description', $request->app_description, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('tahun_anggaran')) {
            $setting = AppSetting::setValue('tahun_anggaran', $request->tahun_anggaran, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('chat_provider')) {
            $setting = AppSetting::setValue('chat_provider', $request->chat_provider, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('chat_base_url')) {
            $setting = AppSetting::setValue('chat_base_url', $request->chat_base_url, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('chat_model')) {
            $setting = AppSetting::setValue('chat_model', $request->chat_model, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('chat_api_key') && filled($request->input('chat_api_key'))) {
            $setting = AppSetting::setValue('chat_api_key_local', $request->input('chat_api_key'), 'secret');
            $updatedSettings[] = $setting;
        }

        foreach ($apiKeyProviders as $providerId) {
            $settingKey = $this->chatApiKeySettingKey($providerId);

            if ($request->has($settingKey) && filled($request->input($settingKey))) {
                $setting = AppSetting::setValue($settingKey, $request->input($settingKey), 'secret');
                $updatedSettings[] = $setting;
            }
        }

        if ($request->has('landing_page_active')) {
            $setting = AppSetting::setValue('landing_page_active', $request->landing_page_active, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('spm_detail_page_active')) {
            $setting = AppSetting::setValue('spm_detail_page_active', $request->spm_detail_page_active, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('puspen_progress_fisik_public')) {
            $setting = AppSetting::setValue('puspen_progress_fisik_public', $request->puspen_progress_fisik_public, 'text');
            $updatedSettings[] = $setting;
        }

        // Handle file uploads
        if ($request->hasFile('logo')) {
            $setting = AppSetting::updateOrCreate(
                ['key' => 'logo'],
                ['type' => 'file', 'value' => null]
            );
            $setting->clearMediaCollection('app-settings');
            $setting->addMediaFromRequest('logo')
                ->usingFileName('logo_' . Str::uuid() . '.' . $request->file('logo')->getClientOriginalExtension())
                ->toMediaCollection('app-settings');
            $updatedSettings[] = $setting->fresh();
        }

        if ($request->hasFile('favicon')) {
            $setting = AppSetting::updateOrCreate(
                ['key' => 'favicon'],
                ['type' => 'file', 'value' => null]
            );
            $setting->clearMediaCollection('app-settings');
            $setting->addMediaFromRequest('favicon')
                ->usingFileName('favicon_' . Str::uuid() . '.' . $request->file('favicon')->getClientOriginalExtension())
                ->toMediaCollection('app-settings');
            $updatedSettings[] = $setting->fresh();
        }

        // Return all settings
        $allSettings = AppSetting::all();
        return AppSettingResource::collection($allSettings);
    }

    /**
     * Test AI endpoint using stored settings (API key read from database unless overridden).
     */
    public function testAiConnection(Request $request)
    {
        $request->validate([
            'base_url' => 'nullable|string|max:255',
            'model' => 'nullable|string|max:128',
            'api_key' => 'nullable|string|max:2000',
        ]);

        $baseUrl = rtrim(
            (string) ($request->input('base_url') ?: AppSetting::getValue('chat_base_url', '')),
            '/'
        );
        $model = (string) ($request->input('model') ?: AppSetting::getValue('chat_model', 'gc/gemini-2.5-flash'));
        $apiKey = $request->input('api_key') ?: AppSetting::getValue('chat_api_key_local');

        if ($baseUrl === '' || !filter_var($baseUrl, FILTER_VALIDATE_URL)) {
            return response()->json(['ok' => false, 'error' => 'URL tidak valid.'], 400);
        }

        if (!filled($apiKey)) {
            return response()->json([
                'ok' => false,
                'error' => 'API key belum tersimpan. Isi field API Key lalu klik Simpan.',
            ], 400);
        }

        $headers = [
            'Accept' => 'application/json',
            'Content-Type' => 'application/json',
            'Authorization' => 'Bearer ' . $apiKey,
        ];

        try {
            $modelsResponse = Http::withHeaders($headers)
                ->timeout(10)
                ->get($baseUrl . '/models');

            if (!$modelsResponse->successful()) {
                return response()->json([
                    'ok' => false,
                    'stage' => 'models',
                    'error' => 'HTTP ' . $modelsResponse->status() . ': ' . substr($modelsResponse->body(), 0, 120),
                ], 422);
            }

            if ($model === '') {
                return response()->json(['ok' => true, 'stage' => 'models', 'used_stored_key' => !$request->filled('api_key')]);
            }

            $chatResponse = Http::withHeaders($headers)
                ->timeout(20)
                ->post($baseUrl . '/chat/completions', [
                    'model' => $model,
                    'messages' => [['role' => 'user', 'content' => 'ping']],
                    'max_tokens' => 1,
                    'stream' => false,
                ]);

            if ($chatResponse->successful()) {
                return response()->json([
                    'ok' => true,
                    'stage' => 'chat',
                    'model' => $model,
                    'used_stored_key' => !$request->filled('api_key'),
                ]);
            }

            return response()->json([
                'ok' => false,
                'stage' => 'chat',
                'model' => $model,
                'error' => $this->formatAiGatewayError($chatResponse->body(), $model),
                'used_stored_key' => !$request->filled('api_key'),
            ], 422);
        } catch (\Throwable $e) {
            return response()->json([
                'ok' => false,
                'error' => 'Gagal terhubung: ' . $e->getMessage(),
            ], 502);
        }
    }

    /**
     * Get storage statistics (sizes of photos, files, and database)
     */
    public function storageStats()
    {
        // Use database summation for media sizes (much more accurate for Spatie Media Library)
        $fotoSize = \Illuminate\Support\Facades\DB::table('media')
            ->where('collection_name', 'foto/pekerjaan')
            ->sum('size') ?? 0;
            
        $fotoCount = \Illuminate\Support\Facades\DB::table('media')
            ->where('collection_name', 'foto/pekerjaan')
            ->count();
            
        $berkasSize = \Illuminate\Support\Facades\DB::table('media')
            ->where('collection_name', 'berkas/dokumen')
            ->sum('size') ?? 0;

        $berkasCount = \Illuminate\Support\Facades\DB::table('media')
            ->where('collection_name', 'berkas/dokumen')
            ->count();

        // Database size (MySQL)
        $dbName = config('database.connections.mysql.database');
        $dbSize = 0;
        try {
            $dbSizeResult = \Illuminate\Support\Facades\DB::select("
                SELECT SUM(data_length + index_length) AS size 
                FROM information_schema.TABLES 
                WHERE table_schema = ?
            ", [$dbName]);
            $dbSize = (float)($dbSizeResult[0]->size ?? 0);
        } catch (\Exception $e) {
            // Fallback for other DBs if needed
        }

        return response()->json([
            'data' => [
                'foto' => (float)$fotoSize,
                'foto_count' => $fotoCount,
                'berkas' => (float)$berkasSize,
                'berkas_count' => $berkasCount,
                'database' => $dbSize,
                'media_total' => (float)($fotoSize + $berkasSize),
                'app_total' => (float)($fotoSize + $berkasSize + $dbSize)
            ]
        ]);
    }

    private function getDirSize($directory)
    {
        if (!file_exists($directory) || !is_dir($directory)) return 0;
        
        $size = 0;
        try {
            $files = new \RecursiveIteratorIterator(
                new \RecursiveDirectoryIterator($directory, \RecursiveDirectoryIterator::SKIP_DOTS)
            );
            foreach ($files as $file) {
                $size += $file->getSize();
            }
        } catch (\Exception $e) {
            // Handle inaccessible files/directories
        }
        return $size;
    }

    private function chatApiKeySettingKey(string $providerId): string
    {
        return 'chat_api_key_' . str_replace('-', '_', $providerId);
    }

    private function formatAiGatewayError(string $raw, string $model): string
    {
        $decoded = json_decode($raw, true);
        if (is_array($decoded)) {
            $message = $decoded['message'] ?? $decoded['error'] ?? null;
            $payloadModel = is_string($decoded['model'] ?? null) ? $decoded['model'] : $model;

            if (is_string($message) && stripos($message, 'blocked') !== false) {
                return 'Model "' . $payloadModel . '" diblokir gateway AI. Ganti ke gc/gemini-2.5-flash.';
            }

            if (is_string($message) && $message !== '') {
                return 'Model "' . $payloadModel . '": ' . $message;
            }
        }

        $trimmed = trim($raw);

        return $trimmed !== '' ? substr($trimmed, 0, 200) : 'Model "' . $model . '" gagal diuji.';
    }
}
