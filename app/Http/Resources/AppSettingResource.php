<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class AppSettingResource extends JsonResource
{
    public function toArray($request)
    {
        $value = $this->value;

        if (str_starts_with((string) $this->key, 'chat_api_key_')) {
            $value = null;
        }

        // If the setting is a file type, return the media URL
        if ($this->type === 'file') {
            $media = $this->getFirstMedia('app-settings');
            $value = $media ? $media->getUrl() : null;
        }

        return [
            'id' => $this->id,
            'key' => $this->key,
            'value' => $value,
            'type' => $this->type,
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
