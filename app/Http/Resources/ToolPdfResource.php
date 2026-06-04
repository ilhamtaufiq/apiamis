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
            'signature_placements' => $this->whenLoaded('signaturePlacements', fn () => $this->signaturePlacements->map(fn ($placement) => [
                'id' => (string) $placement->id,
                'signature_id' => $placement->signature_id,
                'page_number' => $placement->page_number,
                'x_ratio' => (float) $placement->x_ratio,
                'y_ratio' => (float) $placement->y_ratio,
                'scale' => (float) $placement->scale,
                'sort_order' => $placement->sort_order,
                'signature_name' => $placement->signature_name,
                'signature_file_name' => $placement->signature_file_name,
                'signature_mime_type' => $placement->signature_mime_type,
                'signature_width' => $placement->signature_width,
                'signature_height' => $placement->signature_height,
                'signature_data_url' => $placement->signature_data_url,
                'signature_source_type' => $placement->signature_source_type,
                'signature_source_id' => $placement->signature_source_id,
            ])),
            'created_at' => $this->created_at?->toISOString(),
            'updated_at' => $this->updated_at?->toISOString(),
        ];
    }
}
