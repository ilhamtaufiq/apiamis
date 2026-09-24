<?php

return [
    'url' => env('PAPERLESS_URL', 'http://localhost:8000'),
    'token' => env('PAPERLESS_TOKEN', ''),
    'auto_sync' => env('PAPERLESS_AUTO_SYNC', false),
    'queue' => env('PAPERLESS_QUEUE', 'default'),
    'allowed_mimes' => [
        'application/pdf',
        'image/png',
        'image/jpeg',
        'image/tiff',
        'application/msword',
        'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    ],
];
