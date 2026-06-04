<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class PuspenMediaShareResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        $mediaItems = $this->getMedia('shared-media');
        $media = $mediaItems->first();
        $files = $mediaItems->map(fn ($item) => [
            'id' => (string) $item->id,
            'name' => $item->name,
            'file_name' => $item->file_name,
            'mime_type' => $item->mime_type,
            'size' => $item->size,
            'url' => $item->getFullUrl(),
            'preview_url' => url("/api/public/puspen/media-shares/{$this->share_token}/preview/{$item->id}"),
            'folder_path' => $item->getCustomProperty('folder_path'),
        ])->values();

        return [
            'id' => (string) $this->id,
            'title' => $this->title,
            'description' => $this->description,
            'share_token' => $this->share_token,
            'is_public' => $this->is_public,
            'expires_at' => $this->expires_at?->toISOString(),
            'download_count' => $this->download_count,
            'last_downloaded_at' => $this->last_downloaded_at?->toISOString(),
            'files' => $files,
            'file' => $media ? [
                'id' => (string) $media->id,
                'name' => $media->name,
                'file_name' => $media->file_name,
                'mime_type' => $media->mime_type,
                'size' => $media->size,
                'url' => $media->getFullUrl(),
                'preview_url' => url("/api/public/puspen/media-shares/{$this->share_token}/preview/{$media->id}"),
                'folder_path' => $media->getCustomProperty('folder_path'),
            ] : null,
            'download_url' => url("/api/public/puspen/media-shares/{$this->share_token}/download"),
            'created_at' => $this->created_at?->toISOString(),
            'updated_at' => $this->updated_at?->toISOString(),
        ];
    }
}
