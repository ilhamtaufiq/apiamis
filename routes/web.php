<?php

use Illuminate\Support\Facades\Route;

Route::get('/', function () {
    $payload = [
        'service' => config('app.name', 'Arumanis API'),
        'status' => 'ok',
        'health' => url('/up'),
        'api' => url('/api'),
    ];

    return response()->json($payload);
});
