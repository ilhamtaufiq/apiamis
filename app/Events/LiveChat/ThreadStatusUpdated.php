<?php

namespace App\Events\LiveChat;

use App\Http\Resources\LiveChatThreadResource;
use App\Models\LiveChatThread;
use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Foundation\Events\Dispatchable;
use Illuminate\Queue\SerializesModels;

class ThreadStatusUpdated implements ShouldBroadcastNow
{
    use Dispatchable, InteractsWithSockets, SerializesModels;

    public function __construct(public LiveChatThread $thread)
    {
        $this->thread->loadMissing(['user', 'latestMessage.user']);
    }

    public function broadcastOn(): array
    {
        return [
            new PrivateChannel('live-chat.thread.'.$this->thread->id),
        ];
    }

    public function broadcastAs(): string
    {
        return 'thread.status';
    }

    public function broadcastWith(): array
    {
        return [
            'thread' => (new LiveChatThreadResource($this->thread))->resolve(),
        ];
    }
}