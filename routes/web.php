<?php

use Illuminate\Support\Facades\Route;

Route::get('/', function () {
    return response()->json([
        'service' => config('app.name', 'Arumanis API'),
        'status' => 'ok',
        'docs' => url('/api/documentation'),
        'health' => url('/up'),
        'api' => url('/api'),
    ]);
});
