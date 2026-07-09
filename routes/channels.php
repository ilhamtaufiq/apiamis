<?php

use App\Models\LiveChatThread;
use App\Models\Pekerjaan;
use Illuminate\Support\Facades\Broadcast;

Broadcast::channel('App.Models.User.{id}', function ($user, $id) {
    return (int) $user->id === (int) $id;
});

Broadcast::channel('pekerjaan.{pekerjaanId}', function ($user, $pekerjaanId) {
    return Pekerjaan::query()
        ->byUserRole()
        ->whereKey($pekerjaanId)
        ->exists();
});

Broadcast::channel('live-chat.thread.{threadId}', function ($user, $threadId) {
    $thread = LiveChatThread::query()->find($threadId);

    if (! $thread) {
        return false;
    }

    return $user->hasRole('admin') || (int) $thread->user_id === (int) $user->id;
});

Broadcast::channel('live-chat.inbox', function ($user) {
    return $user->hasRole('admin');
});
