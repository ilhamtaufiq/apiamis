<?php

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
