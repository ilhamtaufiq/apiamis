<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;
use App\Traits\Auditable;

class AppSetting extends Model implements HasMedia
{
    use InteractsWithMedia, Auditable;

    protected $table = 'app_settings';

    protected $fillable = [
        'key',
        'value',
        'type',
    ];

    /**
     * Judul berkas yang bisa dibuka ke role pengawas / konsultan_pengawas
     * lewat toggle pengaturan (1 = tampilkan).
     *
     * @var array<string, string>
     */
    public const PENGAWAS_BERKAS_JUDUL_KEYS = [
        'RAB' => 'pengawas_berkas_show_rab',
        'GAMBAR' => 'pengawas_berkas_show_gambar',
        'NEGO' => 'pengawas_berkas_show_nego',
    ];

    /**
     * Get a setting value by key
     */
    public static function getValue(string $key, $default = null): ?string
    {
        $setting = static::where('key', $key)->first();
        return $setting ? $setting->value : $default;
    }

    /**
     * Judul berkas (RAB / GAMBAR / NEGO) yang diaktifkan untuk role lapangan.
     *
     * @return list<string>
     */
    public static function pengawasVisibleBerkasJuduls(): array
    {
        $titles = [];

        foreach (self::PENGAWAS_BERKAS_JUDUL_KEYS as $judul => $key) {
            if (self::getValue($key, '0') === '1') {
                $titles[] = $judul;
            }
        }

        return $titles;
    }

    /**
     * Set a setting value by key
     */
    public static function setValue(string $key, ?string $value, string $type = 'text'): self
    {
        return static::updateOrCreate(
            ['key' => $key],
            ['value' => $value, 'type' => $type]
        );
    }

    /**
     * Get all settings as key-value array
     */
    public static function getAllSettings(): array
    {
        $settings = static::all();
        $result = [];

        foreach ($settings as $setting) {
            if ($setting->type === 'file') {
                $media = $setting->getFirstMedia('app-settings');
                $result[$setting->key] = $media ? $media->getUrl() : null;
            } else {
                $result[$setting->key] = $setting->value;
            }
        }

        return $result;
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('app-settings')
            ->singleFile();
    }
}
