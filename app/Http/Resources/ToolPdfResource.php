<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class ToolPdfResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => (string) $this->id,
            'name' => $this->name,
            'original_filename' => $this->original_filename,
            'kind' => $this->kind,
            'parent_id' => $this->parent_id ? (string) $this->parent_id : null,
            'pdf_url' => $this->pdf_url,
            'created_at' => $this->created_at?->toISOString(),
            'updated_at' => $this->updated_at?->toISOString(),
        ];
    }
}
