<?php

namespace App\Services;

use App\Models\Pekerjaan;
use Dompdf\Dompdf;
use Dompdf\Options;
use Illuminate\Support\Facades\View;

/**
 * Generate laporan PDF pekerjaan (kop Disperkim) untuk tautan unduh
 * dari jawaban Asisten AI. PDF dibangun saat tombol unduh diklik —
 * tool hanya memvalidasi akses dan mengembalikan URL-nya.
 */
class PekerjaanReportPdfService
{
    /** Batas baris tabel rekap agar render dompdf tetap ringan. */
    private const REKAP_MAX_ROWS = 300;

    /** Batas thumbnail foto dokumentasi di laporan paket. */
    private const PAKET_MAX_FOTO = 4;

    public function generatePaket(int $pekerjaanId): array
    {
        $project = Pekerjaan::byUserRole()
            ->with([
                'kontrak.penyedia', 'kontrak.addendums', 'kontrakLegacy.penyedia',
                'kontrakLegacy.addendums', 'progressEstimasiHistory', 'kecamatan', 'desa',
                'kegiatan', 'tiket', 'output', 'penerima', 'foto', 'pengawas', 'pendamping',
            ])
            ->find($pekerjaanId);

        if (!$project) {
            throw new \RuntimeException('Paket tidak ditemukan atau bukan wewenang Anda.');
        }

        $tahun = (int) ($project->kegiatan->tahun_anggaran ?? now()->year);
        $estimasi = app(PekerjaanProgressEstimasiSummaryService::class)
            ->summarize($project->progressEstimasiHistory, $tahun);

        $kontraks = $project->kontrak->concat($project->kontrakLegacy)->unique('id')->values();
        $tiketOpen = $project->tiket->where('status', 'open')->count();

        $html = View::make('exports.laporan-paket-pdf', [
            'logoDataUrl' => $this->logoDataUrl(),
            'judul' => 'LAPORAN PEKERJAAN',
            'generatedAt' => now()->format('d/m/Y H:i'),
            'tahun' => $tahun,
            'paket' => [
                'nama' => $project->nama_paket,
                'status' => $this->statusLabel($project->status ?? 'active'),
                'pagu' => (float) $project->pagu,
                'lokasi' => trim(($project->desa->n_desa ?? '') . ', ' . ($project->kecamatan->n_kec ?? ''), ', '),
                'kegiatan' => $project->kegiatan?->nama_kegiatan,
                'sub_kegiatan' => $project->kegiatan?->nama_sub_kegiatan,
                'sumber_dana' => $project->kegiatan?->sumber_dana,
                'pptk' => $project->kegiatan?->nama_pptk,
                'pengawas' => $project->pengawas->nama ?? null,
                'pendamping' => $project->pendamping->nama ?? null,
                'catatan' => $project->catatan,
            ],
            'progres' => [
                'fisik' => $estimasi['fisik_realisasi'],
                'keuangan' => $estimasi['keuangan_realisasi'],
                'fisik_deviasi' => $estimasi['fisik_deviasi'],
                'keuangan_deviasi' => $estimasi['keuangan_deviasi'],
            ],
            'kontraks' => $kontraks->map(fn($k) => [
                'spk' => $k->spk ?? '-',
                'penyedia' => $k->penyedia->nama ?? 'N/A',
                'nilai' => (float) $k->nilai_kontrak,
                'nilai_berjalan' => $k->nilaiKontrakBerjalan(),
                'tgl_spk' => $k->tgl_spk?->format('d/m/Y'),
                'tgl_selesai' => $k->tglSelesaiBerjalan()?->format('d/m/Y'),
                'addendum_count' => ($k->addendums ?? collect())->count(),
            ])->all(),
            'outputs' => $project->output->map(fn($o) => [
                'komponen' => $o->komponen,
                'satuan' => $o->satuan,
                'volume' => (float) $o->volume,
            ])->all(),
            'penerima' => $project->penerima->map(fn($p) => [
                'nama' => $p->nama,
                'jumlah_jiwa' => (int) $p->jumlah_jiwa,
                'alamat' => $p->alamat,
            ])->take(20)->all(),
            'penerima_total' => $project->penerima->count(),
            'penerima_jiwa' => (int) $project->penerima->sum('jumlah_jiwa'),
            'tiket_total' => $project->tiket->count(),
            'tiket_open' => $tiketOpen,
            'foto_count' => $project->foto->count(),
            'fotos' => $this->fotoDataUrls($project),
        ])->render();

        return [
            'filename' => $this->filename('laporan-paket', $project->nama_paket),
            'pdf' => $this->render($html, 'a4', 'portrait'),
        ];
    }

    public function generateRekap(?int $tahun = null, ?string $kecamatan = null): array
    {
        $query = Pekerjaan::byUserRole()
            ->with(['kegiatan', 'kecamatan', 'desa', 'kontrak.penyedia', 'kontrakLegacy.penyedia']);

        if ($tahun !== null) {
            $query->whereHas('kegiatan', fn($q) => $q->where('tahun_anggaran', $tahun));
        }

        if ($kecamatan) {
            $query->whereHas('kecamatan', fn($q) => $q->where('n_kec', 'LIKE', "%{$kecamatan}%"));
        }

        $projects = $query->get();
        if ($projects->isEmpty()) {
            throw new \RuntimeException('Tidak ada paket untuk filter yang diberikan.');
        }

        $estimasiService = app(PekerjaanProgressEstimasiSummaryService::class);
        $fisikValues = $projects
            ->map(fn($p) => $estimasiService
                ->summarize($p->progressEstimasiHistory, (int) ($p->kegiatan->tahun_anggaran ?? $tahun ?? now()->year))['fisik_realisasi'] ?? null)
            ->filter(fn($v) => $v !== null);

        $rows = $projects->take(self::REKAP_MAX_ROWS)->map(fn($p) => [
            'nama' => $p->nama_paket,
            'lokasi' => trim(($p->desa->n_desa ?? '') . ', ' . ($p->kecamatan->n_kec ?? ''), ', '),
            'pagu' => (float) $p->pagu,
            'fisik' => $estimasiService
                ->summarize($p->progressEstimasiHistory, (int) ($p->kegiatan->tahun_anggaran ?? $tahun ?? now()->year))['fisik_realisasi'],
            'kontrak' => $p->kontrak->concat($p->kontrakLegacy)->first()?->spk ?? 'Belum ada',
            'status' => $this->statusLabel($p->status ?? 'active'),
        ])->values();

        $html = View::make('exports.laporan-rekap-pdf', [
            'logoDataUrl' => $this->logoDataUrl(),
            'judul' => 'LAPORAN REKAP PEKERJAAN',
            'generatedAt' => now()->format('d/m/Y H:i'),
            'tahun' => $tahun,
            'kecamatan' => $kecamatan,
            'stats' => [
                'total' => $projects->count(),
                'total_pagu' => (float) $projects->sum('pagu'),
                'rata_fisik' => $fisikValues->count() > 0 ? round($fisikValues->avg(), 2) : 0,
                'dipotret' => min($projects->count(), self::REKAP_MAX_ROWS),
            ],
            'rows' => $rows->all(),
        ])->render();

        $suffix = $kecamatan ? '-' . \Str::slug($kecamatan) : '';
        $suffix .= $tahun ? "-{$tahun}" : '';

        return [
            'filename' => $this->filename('laporan-rekap-pekerjaan' . $suffix),
            'pdf' => $this->render($html, 'a4', 'landscape'),
        ];
    }

    private function render(string $html, string $paper, string $orientation): string
    {
        $options = new Options;
        $options->set('isRemoteEnabled', false);
        $options->set('defaultFont', 'DejaVu Sans');

        $dompdf = new Dompdf($options);
        $dompdf->loadHtml($html);
        $dompdf->setPaper($paper, $orientation);
        $dompdf->render();

        // Footer tiap halaman: label + nomor halaman (placeholder CPDF).
        $canvas = $dompdf->getCanvas();
        $pageW = $canvas->get_width();
        $margin = 14;
        $y = $canvas->get_height() - 10;

        $canvas->page_text(
            $margin, $y,
            'AI Generated · Bidang Air Minum dan Sanitasi · Disperkim Cianjur',
            'helvetica', 7, [100, 116, 139],
        );
        $canvas->page_text(
            $pageW - $margin - 60, $y,
            'Halaman {PAGE_NUM} / {PAGE_COUNT}',
            'helvetica', 7, [100, 116, 139],
        );

        return $dompdf->output();
    }

    private function logoDataUrl(): string
    {
        $path = public_path('logo-cianjurkab.png');
        $bytes = is_readable($path) ? (string) file_get_contents($path) : '';

        return $bytes !== '' ? 'data:image/png;base64,' . base64_encode($bytes) : '';
    }

    /** Thumbnail foto dokumentasi sebagai data URI (aman dari chroot dompdf). */
    private function fotoDataUrls(Pekerjaan $project): array
    {
        return $project->foto->take(self::PAKET_MAX_FOTO)->map(function ($foto) {
            $path = null;
            try {
                $path = $foto->getFirstMediaPath('foto/pekerjaan', 'thumb')
                    ?: $foto->getFirstMediaPath('foto/pekerjaan');
            } catch (\Throwable) {
                $path = null;
            }

            if (!$path || !is_readable($path) || filesize($path) > 3 * 1024 * 1024) {
                return null;
            }

            $mime = str_ends_with(strtolower($path), '.png') ? 'image/png' : 'image/jpeg';

            return [
                'src' => 'data:' . $mime . ';base64,' . base64_encode((string) file_get_contents($path)),
                'keterangan' => $foto->keterangan,
            ];
        })->filter()->values()->all();
    }

    private function statusLabel(string $status): string
    {
        return match ($status) {
            'canceled' => 'Batal',
            'active' => 'Berjalan',
            default => ucfirst($status),
        };
    }

    private function filename(string $prefix, ?string $subject = null): string
    {
        $name = $prefix;
        if ($subject) {
            $name .= '-' . \Str::slug(mb_substr($subject, 0, 60));
        }

        return $name . '-' . now()->format('Ymd_His') . '.pdf';
    }
}
