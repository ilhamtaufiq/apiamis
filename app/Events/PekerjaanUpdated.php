<?php

namespace App\Events;

use App\Models\Pekerjaan;
use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Foundation\Events\Dispatchable;
use Illuminate\Queue\SerializesModels;

class PekerjaanUpdated implements ShouldBroadcastNow
{
    use Dispatchable, InteractsWithSockets, SerializesModels;

    public function __construct(
        public int $pekerjaanId,
        public string $resource,
        public string $action,
        public ?int $resourceId = null,
    ) {}

    /**
     * @return array<int, \Illuminate\Broadcasting\Channel>
     */
    public function broadcastOn(): array
    {
        $channels = [
            new PrivateChannel('pekerjaan.'.$this->pekerjaanId),
        ];

        $pekerjaan = Pekerjaan::query()
            ->with('assignedUsers:id')
            ->find($this->pekerjaanId);

        if ($pekerjaan) {
            foreach ($pekerjaan->assignedUsers as $user) {
                $channels[] = new PrivateChannel('App.Models.User.'.$user->id);
            }
        }

        return $channels;
    }

    public function broadcastAs(): string
    {
        return 'pekerjaan.updated';
    }

    /**
     * @return array<string, mixed>
     */
    public function broadcastWith(): array
    {
        return [
            'pekerjaan_id' => $this->pekerjaanId,
            'resource' => $this->resource,
            'action' => $this->action,
            'resource_id' => $this->resourceId,
            'updated_at' => now()->toIso8601String(),
        ];
    }
}