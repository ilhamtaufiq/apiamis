<!DOCTYPE html>
<html lang="id">
<head>
    <meta charset="utf-8">
    <title>{{ $judul }}</title>
    <style>
        body { font-family: DejaVu Sans, sans-serif; font-size: 9px; color: #0f172a; }
        h1 { font-size: 13px; text-align: center; margin: 10px 0 2px; letter-spacing: 1px; }
        .subtitle { text-align: center; font-size: 10px; margin: 0 0 2px; }
        .meta { text-align: center; font-size: 8px; color: #64748b; margin-bottom: 10px; }
        h2 { font-size: 10px; margin: 12px 0 4px; padding: 2px 6px; background: #eff6ff;
             border-left: 3px solid #2563eb; }
        .kop { width: 100%; border-collapse: collapse; }
        .kop td { vertical-align: middle; padding: 0; }
        .kop-logo { width: 70px; text-align: center; }
        .kop-text { text-align: center; }
        .kop-line1 { font-size: 11px; font-weight: bold; }
        .kop-line2 { font-size: 11px; font-weight: bold; }
        .kop-line3 { font-size: 10px; font-style: italic; }
        .kop-rule { border-bottom: 2px solid #0f172a; margin-top: 4px; margin-bottom: 2px; }
        table { width: 100%; border-collapse: collapse; margin-top: 2px; }
        th, td { border: 0.4px solid #cbd5e1; padding: 3px 5px; vertical-align: top; }
        th { background: #f1f5f9; font-weight: bold; text-align: center; }
        tr:nth-child(even) td { background: #f8fafc; }
        .label { width: 22%; background: #f8fafc; font-weight: bold; }
        .num { text-align: right; }
        .center { text-align: center; }
        .muted { color: #64748b; font-size: 8px; }
        .foto-grid { width: 100%; border-collapse: collapse; }
        .foto-grid td { border: none; text-align: center; padding: 4px; }
        .foto-grid img { width: 120px; height: 90px; }
        .foto-cap { font-size: 7.5px; color: #475569; }
    </style>
</head>
<body>
    @include('exports.laporan-kop-pdf')

    <h1>{{ $judul }}</h1>
    <p class="subtitle">{{ $paket['nama'] }}</p>
    <div class="meta">
        Tahun Anggaran {{ $tahun }} · Dicetak: {{ $generatedAt }} · Arumanis
    </div>

    <h2>A. Identitas Paket</h2>
    <table>
        <tr><td class="label">Nama Paket</td><td>{{ $paket['nama'] }}</td></tr>
        <tr><td class="label">Status</td><td>{{ $paket['status'] }}</td></tr>
        <tr><td class="label">Lokasi</td><td>{{ $paket['lokasi'] ?: '-' }}</td></tr>
        <tr><td class="label">Pagu Anggaran</td><td class="num">Rp {{ number_format($paket['pagu'], 0, ',', '.') }}</td></tr>
        <tr><td class="label">Kegiatan</td><td>{{ $paket['kegiatan'] ?? '-' }}</td></tr>
        <tr><td class="label">Sub Kegiatan</td><td>{{ $paket['sub_kegiatan'] ?? '-' }}</td></tr>
        <tr><td class="label">Sumber Dana</td><td>{{ $paket['sumber_dana'] ?? '-' }}</td></tr>
        <tr><td class="label">PPTK</td><td>{{ $paket['pptk'] ?? '-' }}</td></tr>
        <tr><td class="label">Pengawas</td><td>{{ $paket['pengawas'] ?? '-' }}</td></tr>
        <tr><td class="label">Pendamping</td><td>{{ $paket['pendamping'] ?? '-' }}</td></tr>
        @if(!empty($paket['catatan']))
            <tr><td class="label">Catatan</td><td>{{ $paket['catatan'] }}</td></tr>
        @endif
    </table>

    <h2>B. Progres (Estimasi {{ $tahun }})</h2>
    <table>
        <tr>
            <th>Fisik Realisasi</th>
            <th>Deviasi Fisik</th>
            <th>Keuangan Realisasi</th>
            <th>Deviasi Keuangan</th>
        </tr>
        <tr>
            <td class="center">{{ $progres['fisik'] ?? '-' }}%</td>
            <td class="center">{{ $progres['fisik_deviasi'] ?? '-' }}%</td>
            <td class="center">{{ $progres['keuangan'] ?? '-' }}%</td>
            <td class="center">{{ $progres['keuangan_deviasi'] ?? '-' }}%</td>
        </tr>
    </table>

    <h2>C. Kontrak / SPK</h2>
    <table>
        <thead>
            <tr>
                <th>No. SPK</th>
                <th>Penyedia</th>
                <th>Nilai Kontrak</th>
                <th>Nilai Berjalan</th>
                <th>Tgl SPK</th>
                <th>Jatuh Tempo</th>
                <th>Addendum</th>
            </tr>
        </thead>
        <tbody>
            @forelse($kontraks as $k)
                <tr>
                    <td>{{ $k['spk'] }}</td>
                    <td>{{ $k['penyedia'] }}</td>
                    <td class="num">Rp {{ number_format($k['nilai'], 0, ',', '.') }}</td>
                    <td class="num">Rp {{ number_format((float) $k['nilai_berjalan'], 0, ',', '.') }}</td>
                    <td class="center">{{ $k['tgl_spk'] ?? '-' }}</td>
                    <td class="center">{{ $k['tgl_selesai'] ?? '-' }}</td>
                    <td class="center">{{ $k['addendum_count'] ?: '-' }}</td>
                </tr>
            @empty
                <tr><td colspan="7" class="center">Belum ada kontrak</td></tr>
            @endforelse
        </tbody>
    </table>

    @if(count($outputs) > 0)
        <h2>D. Output / Komponen + Kelengkapan Foto</h2>
        <table>
            <thead>
                <tr><th style="width:5%">No</th><th>Komponen</th><th style="width:12%">Satuan</th><th style="width:15%">Volume</th><th style="width:18%">Foto (5 slot/unit)</th><th style="width:12%">Status</th></tr>
            </thead>
            <tbody>
                @foreach($outputs as $i => $o)
                    <tr>
                        <td class="center">{{ $i + 1 }}</td>
                        <td>{{ $o['komponen'] }}</td>
                        <td class="center">{{ $o['satuan'] ?? '-' }}</td>
                        <td class="num">{{ number_format($o['volume'], 2, ',', '.') }}</td>
                        <td class="center">{{ $o['foto_done'] }}/{{ $o['foto_target'] }}</td>
                        <td class="center">{{ $o['foto_lengkap'] ? 'LENGKAP' : 'KURANG' }}</td>
                    </tr>
                @endforeach
            </tbody>
        </table>
    @endif

    @if($penerima_total > 0)
        <h2>E. Penerima Manfaat ({{ $penerima_total }} titik · {{ number_format($penerima_jiwa, 0, ',', '.') }} jiwa)</h2>
        <table>
            <thead>
                <tr><th style="width:5%">No</th><th>Nama</th><th style="width:15%">Jiwa</th><th>Alamat</th></tr>
            </thead>
            <tbody>
                @foreach($penerima as $i => $p)
                    <tr>
                        <td class="center">{{ $i + 1 }}</td>
                        <td>{{ $p['nama'] }}</td>
                        <td class="center">{{ number_format($p['jumlah_jiwa'], 0, ',', '.') }}</td>
                        <td>{{ $p['alamat'] ?? '-' }}</td>
                    </tr>
                @endforeach
            </tbody>
        </table>
        @if($penerima_total > count($penerima))
            <p class="muted">Menampilkan {{ count($penerima) }} dari {{ $penerima_total }} penerima.</p>
        @endif
    @endif

    <h2>F. Penilaian Kelengkapan ({{ $penilaian['lengkap'] ? 'LENGKAP' : 'BELUM LENGKAP' }})</h2>
    <table>
        <tr>
            <td class="label">Foto sesuai output</td>
            <td>{{ $penilaian['foto_lengkap'] ? 'Ya — semua komponen memenuhi 5 slot foto per unit' : 'Belum — lihat kolom Foto di tabel Output (D)' }}</td>
        </tr>
        <tr>
            <td class="label">Penerima vs kebutuhan</td>
            <td>
                @if(count($penilaian['penerima_kebutuhan']) > 0)
                    {{ $penilaian['penerima_cukup'] ? 'Cukup' : 'Kurang' }} — terdaftar {{ $penilaian['penerima_total'] }} penerima.
                    @foreach($penilaian['penerima_kebutuhan'] as $k)
                        {{ $k['komponen'] }}: butuh {{ $k['target'] }}, tersedia {{ $k['tersedia'] }}{{ !$loop->last ? ';' : '.' }}
                    @endforeach
                @else
                    Tidak ada output satuan unit yang mensyaratkan penerima (komunal).
                @endif
            </td>
        </tr>
        <tr>
            <td class="label">Koordinat foto</td>
            <td>
                @if($penilaian['koordinat_invalid_count'] > 0)
                    {{ $penilaian['koordinat_invalid_count'] }} foto koordinat INVALID (di luar desa) — wajib diperbaiki di tab Foto.
                    @foreach($penilaian['koordinat_invalid'] as $kf)
                        {{ $kf['keterangan'] ?? 'Foto' }} ({{ $kf['koordinat'] }}){{ !$loop->last ? ';' : '.' }}
                    @endforeach
                @else
                    Tidak ada koordinat invalid.
                    @if($penilaian['koordinat_tanpa'] > 0)
                        {{ $penilaian['koordinat_tanpa'] }} foto tanpa koordinat.
                    @endif
                @endif
            </td>
        </tr>
    </table>

    <h2>G. Catatan Pelaksanaan</h2>
    <table>
        <tr>
            <td class="label">Tiket</td>
            <td>{{ $tiket_total }} total{{ $tiket_open > 0 ? ' · ' . $tiket_open . ' terbuka' : ' · tidak ada yang terbuka' }}</td>
        </tr>
        <tr>
            <td class="label">Dokumentasi</td>
            <td>{{ $foto_count }} foto terdokumentasi</td>
        </tr>
    </table>

    @if(count($fotos) > 0)
        <h2>H. Dokumentasi Foto</h2>
        <table class="foto-grid">
            <tr>
                @foreach($fotos as $f)
                    <td>
                        <img src="{{ $f['src'] }}"><br>
                        <span class="foto-cap">{{ \Illuminate\Support\Str::limit($f['keterangan'] ?? '-', 40) }}</span>
                    </td>
                @endforeach
            </tr>
        </table>
    @endif
</body>
</html>
