<?php

return [

    /*
    |--------------------------------------------------------------------------
    | Third Party Services
    |--------------------------------------------------------------------------
    |
    | This file is for storing the credentials for third party services such
    | as Mailgun, Postmark, AWS and more. This file provides the de facto
    | location for this type of information, allowing packages to have
    | a conventional file to locate the various service credentials.
    |
    */

    'postmark' => [
        'key' => env('POSTMARK_API_KEY'),
    ],

    'resend' => [
        'key' => env('RESEND_API_KEY'),
    ],

    'ses' => [
        'key' => env('AWS_ACCESS_KEY_ID'),
        'secret' => env('AWS_SECRET_ACCESS_KEY'),
        'region' => env('AWS_DEFAULT_REGION', 'us-east-1'),
    ],

    'slack' => [
        'notifications' => [
            'bot_user_oauth_token' => env('SLACK_BOT_USER_OAUTH_TOKEN'),
            'channel' => env('SLACK_BOT_USER_DEFAULT_CHANNEL'),
        ],
    ],

    'google' => [
        'client_id' => env('GOOGLE_CLIENT_ID'),
        'client_secret' => env('GOOGLE_CLIENT_SECRET'),
        'redirect' => env('GOOGLE_REDIRECT_URI'),
    ],

    'whatsapp_bridge' => [
        'url' => env('WHATSAPP_BRIDGE_URL', 'http://127.0.0.1:4000'),
        'key' => env('WHATSAPP_BRIDGE_KEY'),
    ],

    'minimax' => [
        'api_key' => env('VITE_MINIMAX_API_KEY'),
    ],

    'openrouter' => [
        'api_key' => env('OPENROUTER_API_KEY'),
        'model' => env('OPENROUTER_MODEL', 'openai/gpt-oss-120b:free'),
        'fallback_model' => env('OPENROUTER_FALLBACK_MODEL', 'z-ai/glm-4.5-air:free'),
    ],

    'spse' => [
        'base_url' => env('SPSE_BASE_URL', 'https://spse.inaproc.id'),
        'lpse_slug' => env('SPSE_LPSE_SLUG', 'cianjurkab'),
        'ppk_nama' => env('SPSE_PPK_NAMA', 'AGUNG DELI SAHPUTRA, ST'),
        'ppk_nip' => env('SPSE_PPK_NIP', '197711212006041010'),
        'ppk_jabatan' => env('SPSE_PPK_JABATAN', 'Kepala Bidang'),
        'ppk_no_sk' => env('SPSE_PPK_NO_SK', '800.1.3.3/Kep.411/BKPSDM/10/2025'),
        'satker_kota' => env('SPSE_SATKER_KOTA', 'Cianjur'),
        'satker_alamat' => env('SPSE_SATKER_ALAMAT', 'Jl. Adi Sucipta No. 7 - Cianjur'),
        'cara_pembayaran' => env('SPSE_CARA_PEMBAYARAN', 'Sekaligus'),
        'waktu_penyelesaian' => env('SPSE_WAKTU_PENYELESAIAN', '60 Hari Kalender'),
    ],

];
