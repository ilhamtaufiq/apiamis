<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class LiveChatThreadResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'user_id' => $this->user_id,
            'user' => new UserResource($this->whenLoaded('user')),
            'status' => $this->status,
            'last_message_at' => $this->last_message_at,
            'latest_message' => new LiveChatMessageResource($this->whenLoaded('latestMessage')),
            'unread_count' => $this->when(isset($this->unread_count), (int) $this->unread_count),
            'created_at' => $this->created_at,
            'updated_at' => $this->updated_at,
        ];
    }
}