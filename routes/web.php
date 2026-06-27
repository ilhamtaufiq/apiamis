<?php

use Illuminate\Support\Facades\Route;

Route::get('/', function () {
    $payload = [
        'service' => config('app.name', 'Arumanis API'),
        'status' => 'ok',
        'health' => url('/up'),
        'api' => url('/api'),
    ];

    if (! app()->environment('production')) {
        $payload['docs'] = url('/api/documentation');
    }

    return response()->json($payload);
});
