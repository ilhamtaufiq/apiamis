<!DOCTYPE html>
<html lang="id">
<head>
    <meta charset="utf-8">
    <title>{{ $judul }}</title>
    <style>
        body { font-family: DejaVu Sans, sans-serif; font-size: 8.5px; color: #0f172a; }
        h1 { font-size: 13px; text-align: center; margin: 10px 0 2px; letter-spacing: 1px; }
        .meta { text-align: center; font-size: 8px; color: #64748b; margin-bottom: 8px; }
        .kop { width: 100%; border-collapse: collapse; }
        .kop td { vertical-align: middle; padding: 0; }
        .kop-logo { width: 70px; text-align: center; }
        .kop-text { text-align: center; }
        .kop-line1 { font-size: 11px; font-weight: bold; }
        .kop-line2 { font-size: 11px; font-weight: bold; }
        .kop-line3 { font-size: 10px; font-style: italic; }
        .kop-rule { border-bottom: 2px solid #0f172a; margin-top: 4px; margin-bottom: 2px; }
        table.data { width: 100%; border-collapse: collapse; margin-top: 4px; }
        table.data th, table.data td { border: 0.4px solid #cbd5e1; padding: 2.5px 4px; vertical-align: top; }
        table.data th { background: #2563eb; color: #fff; font-weight: bold; text-align: center; }
        table.data tr:nth-child(even) td { background: #f8fafc; }
        table.stats { width: 100%; border-collapse: collapse; margin-top: 4px; margin-bottom: 6px; }
        table.stats td { border: 0.4px solid #cbd5e1; padding: 4px 6px; text-align: center; }
        table.stats .val { font-size: 10px; font-weight: bold; }
        table.stats .lbl { font-size: 7.5px; color: #64748b; }
        .num { text-align: right; }
        .center { text-align: center; }
        .muted { color: #64748b; font-size: 7.5px; margin-top: 4px; }
    </style>
</head>
<body>
    @include('exports.laporan-kop-pdf')

    <h1>{{ $judul }}</h1>
    <div class="meta">
        @if(!empty($kecamatan)) Kecamatan {{ $kecamatan }} · @endif
        @if(!empty($tahun)) Tahun Anggaran {{ $tahun }} · @endif
        Dicetak: {{ $generatedAt }} · Arumanis
    </div>

    <table class="stats">
        <tr>
            <td><span class="val">{{ number_format($stats['total'], 0, ',', '.') }}</span><br><span class="lbl">Total Paket</span></td>
            <td><span class="val">Rp {{ number_format($stats['total_pagu'], 0, ',', '.') }}</span><br><span class="lbl">Total Pagu</span></td>
            <td><span class="val">{{ $stats['rata_fisik'] }}%</span><br><span class="lbl">Rata-rata Fisik</span></td>
        </tr>
    </table>

    <table class="data">
        <thead>
            <tr>
                <th style="width:4%">No</th>
                <th>Nama Paket</th>
                <th style="width:16%">Lokasi</th>
                <th style="width:11%">Pagu (Rp)</th>
                <th style="width:7%">Fisik</th>
                <th style="width:16%">No. SPK</th>
                <th style="width:8%">Status</th>
            </tr>
        </thead>
        <tbody>
            @forelse($rows as $i => $r)
                <tr>
                    <td class="center">{{ $i + 1 }}</td>
                    <td>{{ $r['nama'] }}</td>
                    <td>{{ $r['lokasi'] ?: '-' }}</td>
                    <td class="num">{{ number_format($r['pagu'], 0, ',', '.') }}</td>
                    <td class="center">{{ $r['fisik'] !== null ? $r['fisik'] . '%' : '-' }}</td>
                    <td>{{ $r['kontrak'] }}</td>
                    <td class="center">{{ $r['status'] }}</td>
                </tr>
            @empty
                <tr><td colspan="7" class="center">Tidak ada data</td></tr>
            @endforelse
        </tbody>
    </table>
    @if($stats['dipotret'] < $stats['total'])
        <p class="muted">Menampilkan {{ $stats['dipotret'] }} dari {{ number_format($stats['total'], 0, ',', '.') }} paket.</p>
    @endif
</body>
</html>
