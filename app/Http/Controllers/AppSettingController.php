<?php

namespace App\Http\Controllers;

use App\Models\AppSetting;
use App\Http\Resources\AppSettingResource;
use App\Services\BrandColorService;
use App\Services\KontrakTemplateService;
use App\Services\MailConfigService;
use App\Services\MailContentService;
use App\Services\MailTemplateService;
use App\Services\MaintenanceModeService;
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
     * Lightweight public maintenance status for SPA gate.
     */
    public function maintenanceStatus(Request $request, MaintenanceModeService $maintenance)
    {
        $user = $maintenance->resolveUser($request);

        return response()->json([
            'data' => $maintenance->statusPayload($user),
        ]);
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
        $this->normalizeEmptySettingsInput($request);

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
            'capaian_publik_section_active' => 'nullable|string|in:0,1',
            'puspen_progress_fisik_public' => 'nullable|string|in:0,1',
            'pengawas_berkas_show_rab' => 'nullable|string|in:0,1',
            'pengawas_berkas_show_gambar' => 'nullable|string|in:0,1',
            'pengawas_berkas_show_nego' => 'nullable|string|in:0,1',
            'maintenance_mode' => 'nullable|string|in:0,1',
            'maintenance_bypass_emails' => 'nullable|string|max:1000',
            'mail_enabled' => 'nullable|string|in:0,1',
            'mail_host' => 'nullable|string|max:255',
            'mail_port' => 'nullable|string|max:5',
            'mail_encryption' => 'nullable|string|in:tls,ssl,none',
            'mail_username' => 'nullable|email|max:255',
            'mail_password' => 'nullable|string|max:2000',
            'mail_from_address' => 'nullable|email|max:255',
            'mail_from_name' => 'nullable|string|max:255',
            'contact_email' => 'nullable|email|max:255',
            'mail_body_format' => 'nullable|string|in:plain,markdown,html',
            'mail_subject' => 'nullable|string|max:255',
            'mail_body' => 'nullable|string|max:50000',
            'logo' => 'nullable|file|mimes:jpg,jpeg,png,svg|max:2048',
            'favicon' => 'nullable|file|mimes:jpg,jpeg,png,svg,ico|max:1024',
            'login_cover' => 'nullable|file|mimes:jpg,jpeg,png,webp|max:5120',
            'kontrak_template_spk' => 'nullable|file|mimes:docx|max:10240',
            'kontrak_template_ringkasan' => 'nullable|file|mimes:docx,xlsx|max:10240',
            'kontrak_template_bap' => 'nullable|file|mimes:docx|max:10240',
            'kontrak_template_cover_am' => 'nullable|file|mimes:docx|max:10240',
            'kontrak_template_cover_san' => 'nullable|file|mimes:docx|max:10240',
            'kontrak_nama_ppk' => 'nullable|string|max:255',
            'kontrak_nip_ppk' => 'nullable|string|max:32',
            'kontrak_nama_pptk' => 'nullable|string|max:255',
            'kontrak_nip_pptk' => 'nullable|string|max:32',
            'kontrak_masa_pemeliharaan_hari' => 'nullable|integer|min:1|max:3650',
            'kontrak_skpd' => 'nullable|string|max:255',
            'kontrak_nomor_dpa' => 'nullable|string|max:255',
            'kontrak_tanggal_dpa' => 'nullable|string|max:255',
            'kontrak_cara_pembayaran' => 'nullable|string|in:sekaligus,termin,bulan',
            's3_backup_enabled' => 'nullable|string|in:0,1',
            's3_endpoint' => 'nullable|string|max:255',
            's3_region' => 'nullable|string|max:64',
            's3_bucket' => 'nullable|string|max:64',
            's3_access_key_id' => 'nullable|string|max:128',
            's3_secret_access_key' => 'nullable|string|max:255',
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

        if ($request->has('capaian_publik_section_active')) {
            $setting = AppSetting::setValue('capaian_publik_section_active', $request->capaian_publik_section_active, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('puspen_progress_fisik_public')) {
            $setting = AppSetting::setValue('puspen_progress_fisik_public', $request->puspen_progress_fisik_public, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('penerima_pin')) {
            $setting = AppSetting::setValue('penerima_pin', $request->penerima_pin, 'secret');
            $updatedSettings[] = $setting;
        }

        foreach (AppSetting::PENGAWAS_BERKAS_JUDUL_KEYS as $settingKey) {
            if ($request->has($settingKey)) {
                $setting = AppSetting::setValue($settingKey, $request->input($settingKey), 'text');
                $updatedSettings[] = $setting;
            }
        }

        if ($request->has('maintenance_mode')) {
            $setting = AppSetting::setValue('maintenance_mode', $request->maintenance_mode, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('maintenance_bypass_emails')) {
            $setting = AppSetting::setValue(
                'maintenance_bypass_emails',
                $request->maintenance_bypass_emails,
                'text'
            );
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_enabled')) {
            $setting = AppSetting::setValue('mail_enabled', $request->mail_enabled, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_host')) {
            $setting = AppSetting::setValue('mail_host', $request->mail_host, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_port')) {
            $setting = AppSetting::setValue('mail_port', $request->mail_port, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_encryption')) {
            $setting = AppSetting::setValue('mail_encryption', $request->mail_encryption, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_username')) {
            $setting = AppSetting::setValue('mail_username', $request->mail_username, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_password') && filled($request->input('mail_password'))) {
            $setting = AppSetting::setValue('mail_password', $request->input('mail_password'), 'secret');
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_from_address')) {
            $setting = AppSetting::setValue('mail_from_address', $request->mail_from_address, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_from_name')) {
            $setting = AppSetting::setValue('mail_from_name', $request->mail_from_name, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('contact_email')) {
            $setting = AppSetting::setValue('contact_email', $request->contact_email, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_body_format')) {
            $setting = AppSetting::setValue('mail_body_format', $request->mail_body_format, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_subject')) {
            $setting = AppSetting::setValue('mail_subject', $request->mail_subject, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('mail_body')) {
            $setting = AppSetting::setValue('mail_body', $request->mail_body, 'text');
            $updatedSettings[] = $setting;
        }

        foreach ([
            'kontrak_nama_ppk',
            'kontrak_nip_ppk',
            'kontrak_nama_pptk',
            'kontrak_nip_pptk',
            'kontrak_skpd',
            'kontrak_nomor_dpa',
            'kontrak_tanggal_dpa',
            'kontrak_cara_pembayaran',
        ] as $kontrakSettingKey) {
            if ($request->has($kontrakSettingKey)) {
                $setting = AppSetting::setValue(
                    $kontrakSettingKey,
                    $request->input($kontrakSettingKey),
                    'text'
                );
                $updatedSettings[] = $setting;
            }
        }

        if ($request->has('kontrak_masa_pemeliharaan_hari')) {
            $setting = AppSetting::setValue(
                'kontrak_masa_pemeliharaan_hari',
                $request->input('kontrak_masa_pemeliharaan_hari'),
                'text'
            );
            $updatedSettings[] = $setting;
        }

        if ($request->has('s3_backup_enabled')) {
            $setting = AppSetting::setValue('s3_backup_enabled', $request->input('s3_backup_enabled'), 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('s3_endpoint')) {
            $setting = AppSetting::setValue('s3_endpoint', $request->input('s3_endpoint'), 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('s3_region')) {
            $setting = AppSetting::setValue('s3_region', $request->input('s3_region'), 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('s3_bucket')) {
            $setting = AppSetting::setValue('s3_bucket', $request->input('s3_bucket'), 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('s3_access_key_id')) {
            $setting = AppSetting::setValue('s3_access_key_id', $request->input('s3_access_key_id'), 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('s3_secret_access_key') && filled($request->input('s3_secret_access_key'))) {
            $setting = AppSetting::setValue('s3_secret_access_key', $request->input('s3_secret_access_key'), 'secret');
            $updatedSettings[] = $setting;
        }

        // Handle file uploads
        if ($request->hasFile('logo')) {
            $setting = AppSetting::updateOrCreate(
                ['key' => 'logo'],
                ['type' => 'file', 'value' => null]
            );
            $setting->clearMediaCollection('app-settings');
            $media = $setting->addMediaFromRequest('logo')
                ->usingFileName('logo_' . Str::uuid() . '.' . $request->file('logo')->getClientOriginalExtension())
                ->toMediaCollection('app-settings');
            BrandColorService::syncFromLogoUpload($media);
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

        if ($request->hasFile('login_cover')) {
            $setting = AppSetting::updateOrCreate(
                ['key' => 'login_cover'],
                ['type' => 'file', 'value' => null]
            );
            $setting->clearMediaCollection('app-settings');
            $setting->addMediaFromRequest('login_cover')
                ->usingFileName('login_cover_' . Str::uuid() . '.' . $request->file('login_cover')->getClientOriginalExtension())
                ->toMediaCollection('app-settings');
            $updatedSettings[] = $setting->fresh();
        }

        foreach (KontrakTemplateService::TEMPLATES as $settingKey => $definition) {
            $field = $definition['form_field'];
            if (! $request->hasFile($field)) {
                continue;
            }

            $setting = AppSetting::updateOrCreate(
                ['key' => $settingKey],
                ['type' => 'file', 'value' => null]
            );
            $setting->clearMediaCollection('app-settings');
            $uploadedFile = $request->file($field);
            $extension = strtolower($uploadedFile->getClientOriginalExtension() ?: ($definition['format'] ?? 'docx'));

            $setting->addMediaFromRequest($field)
                ->usingFileName($settingKey.'_'.Str::uuid().'.'.$extension)
                ->toMediaCollection('app-settings');
            $updatedSettings[] = $setting->fresh();
        }

        // Return all settings
        $allSettings = AppSetting::all();
        return AppSettingResource::collection($allSettings);
    }

    /**
     * List kontrak document template metadata for settings UI.
     */
    public function kontrakTemplates()
    {
        $settings = AppSetting::whereIn('key', array_keys(KontrakTemplateService::TEMPLATES))
            ->get()
            ->keyBy('key');

        $data = collect(KontrakTemplateService::TEMPLATES)->map(function (array $definition, string $key) use ($settings) {
            $setting = $settings->get($key);
            $media = $setting?->getFirstMedia('app-settings');

            return [
                'key' => $key,
                'label' => $definition['label'],
                'description' => $definition['description'],
                'default_filename' => $definition['default'],
                'form_field' => $definition['form_field'],
                'format' => $definition['format'] ?? 'docx',
                'has_custom' => $media !== null,
                'filename' => $media?->file_name,
                'updated_at' => $setting?->updated_at?->toIso8601String(),
            ];
        })->values();

        return response()->json(['data' => $data]);
    }

    /**
     * Download active kontrak template (custom upload or default file).
     */
    public function downloadKontrakTemplate(string $key)
    {
        if (! KontrakTemplateService::isValidKey($key)) {
            return response()->json(['message' => 'Template tidak dikenal.'], 404);
        }

        try {
            $path = KontrakTemplateService::resolvePath($key);
            $filename = KontrakTemplateService::downloadFilename($key);

            return response()->download($path, $filename);
        } catch (\Throwable $e) {
            return response()->json(['message' => $e->getMessage()], 404);
        }
    }

    /**
     * List recommended mail templates with stored overrides.
     */
    public function mailTemplates()
    {
        return response()->json(['data' => MailTemplateService::catalog()]);
    }

    /**
     * Persist customized mail templates (JSON map keyed by template id).
     */
    public function storeMailTemplates(Request $request)
    {
        $request->validate([
            'templates' => 'required|array',
            'templates.*.format' => 'nullable|string|in:plain,markdown,html',
            'templates.*.subject' => 'nullable|string|max:255',
            'templates.*.body' => 'nullable|string|max:50000',
        ]);

        /** @var array<string, array{format?: string, subject?: string, body?: string}> $templates */
        $templates = $request->input('templates', []);
        MailTemplateService::saveMany($templates);

        return response()->json([
            'data' => MailTemplateService::catalog(),
            'message' => 'Template email berhasil disimpan.',
        ]);
    }

    /**
     * Send a test email for a specific template key.
     */
    public function testMailTemplate(Request $request, string $key)
    {
        if (! MailTemplateService::isValidKey($key)) {
            return response()->json(['ok' => false, 'error' => 'Template email tidak dikenal.'], 404);
        }

        $request->validate([
            'to' => 'required|email|max:255',
            'format' => 'nullable|string|in:plain,markdown,html',
            'subject' => 'nullable|string|max:255',
            'body' => 'nullable|string|max:50000',
            'mail_password' => 'nullable|string|max:2000',
        ]);

        return $this->sendTestMail(
            (string) $request->input('to'),
            array_filter([
                'template_key' => $key,
                'format' => $request->input('format'),
                'subject' => $request->input('subject'),
                'body' => $request->input('body'),
                'mail_password' => $request->input('mail_password'),
            ], static fn ($value) => $value !== null && $value !== ''),
            $request->filled('mail_password')
        );
    }

    /**
     * Send a test email using stored SMTP settings (password read from database unless overridden).
     */
    public function testMailConnection(Request $request)
    {
        $request->validate([
            'to' => 'required|email|max:255',
            'template_key' => 'nullable|string|max:64',
            'mail_enabled' => 'nullable|string|in:0,1',
            'mail_host' => 'nullable|string|max:255',
            'mail_port' => 'nullable|string|max:5',
            'mail_encryption' => 'nullable|string|in:tls,ssl,none',
            'mail_username' => 'nullable|email|max:255',
            'mail_password' => 'nullable|string|max:2000',
            'mail_from_address' => 'nullable|email|max:255',
            'mail_from_name' => 'nullable|string|max:255',
            'mail_body_format' => 'nullable|string|in:plain,markdown,html',
            'mail_subject' => 'nullable|string|max:255',
            'mail_body' => 'nullable|string|max:50000',
            'format' => 'nullable|string|in:plain,markdown,html',
            'subject' => 'nullable|string|max:255',
            'body' => 'nullable|string|max:50000',
        ]);

        $overrides = array_filter([
            'template_key' => $request->input('template_key', 'smtp_test'),
            'mail_enabled' => $request->input('mail_enabled', AppSetting::getValue('mail_enabled', '0')),
            'mail_host' => $request->input('mail_host'),
            'mail_port' => $request->input('mail_port'),
            'mail_encryption' => $request->input('mail_encryption'),
            'mail_username' => $request->input('mail_username'),
            'mail_from_address' => $request->input('mail_from_address'),
            'mail_from_name' => $request->input('mail_from_name'),
            'mail_body_format' => $request->input('mail_body_format') ?? $request->input('format'),
            'mail_subject' => $request->input('mail_subject') ?? $request->input('subject'),
            'mail_body' => $request->input('mail_body') ?? $request->input('body'),
            'mail_password' => $request->input('mail_password'),
        ], static fn ($value) => $value !== null && $value !== '');

        $overrides['mail_enabled'] = '1';

        return $this->sendTestMail(
            (string) $request->input('to'),
            $overrides,
            $request->filled('mail_password')
        );
    }

    /**
     * @param  array<string, mixed>  $overrides
     */
    private function sendTestMail(string $to, array $overrides, bool $usedFreshPassword): \Illuminate\Http\JsonResponse
    {
        if (! MailConfigService::applyFromSettings($overrides)) {
            return response()->json([
                'ok' => false,
                'error' => 'SMTP belum lengkap. Isi host, username Gmail, dan App Password lalu simpan atau kirim saat uji koneksi.',
            ], 400);
        }

        $content = MailContentService::resolveTestContent($overrides);

        try {
            MailContentService::sendRendered(
                $to,
                $content['subject'],
                $content['body'],
                $content['format']
            );

            return response()->json([
                'ok' => true,
                'to' => $to,
                'format' => $content['format'],
                'template_key' => $overrides['template_key'] ?? 'smtp_test',
                'used_stored_password' => ! $usedFreshPassword,
            ]);
        } catch (\Throwable $e) {
            return response()->json([
                'ok' => false,
                'error' => 'Gagal mengirim email: ' . $e->getMessage(),
                'used_stored_password' => ! $usedFreshPassword,
            ], 422);
        }
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

        $headers = [
            'Accept' => 'application/json',
            'Content-Type' => 'application/json',
        ];

        if (filled($apiKey)) {
            $headers['Authorization'] = 'Bearer ' . $apiKey;
        }

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

    private function normalizeEmptySettingsInput(Request $request): void
    {
        $nullableFields = [
            'mail_username',
            'mail_from_address',
            'contact_email',
            'mail_from_name',
            'mail_host',
            'mail_port',
            'mail_encryption',
            'app_name',
            'app_description',
            'tahun_anggaran',
            'chat_provider',
            'chat_base_url',
            'chat_model',
            'chat_api_key',
            'kontrak_nama_ppk',
            'kontrak_nip_ppk',
            'kontrak_nama_pptk',
            'kontrak_nip_pptk',
            'kontrak_masa_pemeliharaan_hari',
            'kontrak_skpd',
            'kontrak_nomor_dpa',
            'kontrak_tanggal_dpa',
            'kontrak_cara_pembayaran',
        ];

        $normalized = [];
        foreach ($nullableFields as $field) {
            if (! $request->has($field)) {
                continue;
            }

            $value = $request->input($field);
            if (is_string($value) && trim($value) === '') {
                $normalized[$field] = null;
            }
        }

        if ($normalized !== []) {
            $request->merge($normalized);
        }
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
