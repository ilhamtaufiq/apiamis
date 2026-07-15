<!DOCTYPE html>
<html lang="id">
<head>
    <meta charset="utf-8">
    <title>{{ $title }}</title>
    <style>
        body { font-family: DejaVu Sans, sans-serif; font-size: 9px; color: #111; }
        h1 { font-size: 14px; margin: 0 0 4px; }
        .meta { font-size: 9px; color: #444; margin-bottom: 12px; }
        table { width: 100%; border-collapse: collapse; }
        th, td { border: 1px solid #ccc; padding: 4px 5px; vertical-align: top; }
        th { background: #f3f4f6; font-weight: bold; text-align: center; }
        td.center { text-align: center; }
        tr:nth-child(even) td { background: #fafafa; }
    </style>
</head>
<body>
    <h1>{{ $title }}</h1>
    <div class="meta">
        @if(!empty($tahun)) Tahun anggaran: {{ $tahun }} · @endif
        Diekspor: {{ $generatedAt }} · Total baris: {{ count($rows) }}
    </div>

    <table>
        <thead>
            <tr>
                @foreach($headings as $heading)
                    <th>{{ $heading }}</th>
                @endforeach
            </tr>
        </thead>
        <tbody>
            @forelse($rows as $row)
                <tr>
                    @foreach($row as $i => $cell)
                        <td class="{{ $i === 0 || $i > 2 ? 'center' : '' }}">{{ $cell }}</td>
                    @endforeach
                </tr>
            @empty
                <tr>
                    <td colspan="{{ count($headings) }}" class="center">Tidak ada data</td>
                </tr>
            @endforelse
        </tbody>
    </table>
</body>
</html>
