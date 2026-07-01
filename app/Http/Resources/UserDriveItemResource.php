<?php

namespace App\Http\Resources;

use App\Models\UserDriveItem;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class UserDriveItemResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        $fileMedia = $this->kind === UserDriveItem::KIND_FILE
            ? $this->getFirstMedia('drive-file')
            : null;

        return [
            'id' => $this->id,
            'parent_id' => $this->parent_id,
            'name' => $this->name,
            'kind' => $this->kind,
            'original_filename' => $this->original_filename,
            'file_url' => $fileMedia?->getFullUrl(),
            'mime_type' => $fileMedia?->mime_type,
            'file_size' => $fileMedia?->size,
            'media_id' => $fileMedia?->id,
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}