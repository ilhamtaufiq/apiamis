<!DOCTYPE html>
<html lang="id">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Akses Dokumentasi API</title>
    <style>
        body {
            font-family: system-ui, -apple-system, Segoe UI, Roboto, sans-serif;
            background: #f8fafc;
            color: #0f172a;
            margin: 0;
            min-height: 100vh;
            display: grid;
            place-items: center;
        }
        .card {
            max-width: 32rem;
            background: #fff;
            border: 1px solid #e2e8f0;
            border-radius: 12px;
            padding: 1.5rem;
            box-shadow: 0 10px 30px rgba(15, 23, 42, 0.06);
        }
        h1 { font-size: 1.25rem; margin: 0 0 0.75rem; }
        p { margin: 0; line-height: 1.6; color: #475569; }
        .status { font-size: 0.875rem; color: #64748b; margin-top: 1rem; }
    </style>
</head>
<body>
    <div class="card">
        <h1>Dokumentasi API Terkunci</h1>
        <p>{{ $message }}</p>
        <p class="status">HTTP {{ $status }}</p>
    </div>
</body>
</html>