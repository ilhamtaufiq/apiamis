<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class AppSetting extends Model implements HasMedia
{
    use InteractsWithMedia;

    protected $table = 'app_settings';

    protected $fillable = [
        'key',
        'value',
        'type',
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
