<?php

namespace App\Services;

use App\Models\AppSetting;

class KontrakTemplateService
{
    /**
     * @var array<string, array{default: string, label: string, form_field: string, description: string}>
     */
    public const TEMPLATES = [
        'kontrak_template_spk' => [
            'default' => 'SPK_Template.docx',
            'label' => 'SPK / Surat Perintah Kerja',
            'form_field' => 'kontrak_template_spk',
            'description' => 'Template utama generate dokumen kontrak (SPK).',
        ],
        'kontrak_template_ringkasan' => [
            'default' => 'ringkasan_kontrak_template.docx',
            'label' => 'Ringkasan Kontrak',
            'form_field' => 'kontrak_template_ringkasan',
            'description' => 'Template ringkasan kontrak.',
        ],
        'kontrak_template_bap' => [
            'default' => 'bap_template.docx',
            'label' => 'BAP (Berita Acara Pembayaran)',
            'form_field' => 'kontrak_template_bap',
            'description' => 'Template BAP / berita acara pembayaran.',
        ],
        'kontrak_template_cover_am' => [
            'default' => 'cover_kontrak_am.docx',
            'label' => 'Cover Kontrak (Air Minum)',
            'form_field' => 'kontrak_template_cover_am',
            'description' => 'Cover kontrak untuk sub bidang air minum.',
        ],
        'kontrak_template_cover_san' => [
            'default' => 'cover_kontrak_san.docx',
            'label' => 'Cover Kontrak (Sanitasi)',
            'form_field' => 'kontrak_template_cover_san',
            'description' => 'Cover kontrak untuk sub bidang sanitasi.',
        ],
    ];

    public static function isValidKey(string $key): bool
    {
        return isset(self::TEMPLATES[$key]);
    }

    public static function resolvePath(string $settingKey): string
    {
        $definition = self::TEMPLATES[$settingKey] ?? null;

        if (! $definition) {
            throw new \InvalidArgumentException("Unknown kontrak template key: {$settingKey}");
        }

        $setting = AppSetting::where('key', $settingKey)->first();
        if ($setting) {
            $media = $setting->getFirstMedia('app-settings');
            if ($media) {
                $path = $media->getPath();
                if ($path && file_exists($path)) {
                    return $path;
                }
            }
        }

        $defaultPath = storage_path('app/templates/'.$definition['default']);
        if (! file_exists($defaultPath)) {
            throw new \Exception('Template tidak ditemukan: '.$definition['default']);
        }

        return $defaultPath;
    }

    public static function downloadFilename(string $settingKey): string
    {
        $definition = self::TEMPLATES[$settingKey];

        $setting = AppSetting::where('key', $settingKey)->first();
        $media = $setting?->getFirstMedia('app-settings');
        if ($media) {
            return $media->file_name ?: $definition['default'];
        }

        return $definition['default'];
    }
}