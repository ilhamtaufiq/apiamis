<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Illuminate\Support\Carbon;

class EventResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     *
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'user_id' => $this->user_id,
            'title' => $this->title,
            'isAllday' => (bool) $this->is_allday,
            'start' => $this->wibToIso($this->start),
            'end' => $this->wibToIso($this->end),
            'category' => $this->category,
            'location' => $this->location,
            'description' => $this->description,
            'color' => $this->color,
            'backgroundColor' => $this->bg_color,
            'borderColor' => $this->border_color,
            'attachments' => $this->attachments,
            'created_at' => $this->created_at,
            'updated_at' => $this->updated_at,
        ];
    }

    /**
     * Kolom start/end disimpan sebagai jam dinding WIB (lihat EventController::toWibStorage),
     * jadi dibaca ulang sebagai WIB agar instant yang dikembalikan benar.
     */
    private function wibToIso(Carbon $value): string
    {
        return Carbon::createFromFormat('Y-m-d H:i:s', $value->format('Y-m-d H:i:s'), 'Asia/Jakarta')
            ->toISOString();
    }
}
