<?php

return [
    'document_server_url' => rtrim((string) env('ONLYOFFICE_DOCUMENT_SERVER_URL', ''), '/'),

    'jwt_secret' => (string) env('ONLYOFFICE_JWT_SECRET', ''),

    'jwt_header' => (string) env('ONLYOFFICE_JWT_HEADER', 'Authorization'),

    'download_token_ttl_minutes' => (int) env('ONLYOFFICE_DOWNLOAD_TOKEN_TTL', 120),
];