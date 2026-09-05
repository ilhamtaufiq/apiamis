<?php

namespace App\Services;

use App\Models\AppSetting;
use App\Models\Berkas;
use App\Models\Desa;
use App\Models\Event;
use App\Models\Foto;
use App\Models\Kecamatan;
use App\Models\Kegiatan;
use App\Models\Kontrak;
use App\Models\KontrakAddendum;
use App\Models\Output;
use App\Models\Pekerjaan;
use App\Models\PekerjaanProgressEstimasiHistory;
use App\Models\Penerima;
use App\Models\Pengawas;
use App\Models\Penyedia;
use App\Models\SpmSanitasi;
use App\Models\Tag;
use App\Models\Tiket;
use App\Models\UsulanKegiatan;
use App\Models\User;
use Illuminate\Http\Request;

class ChatDataToolService
{
    public function __construct(
        private readonly PekerjaanProgressEstimasiSummaryService $estimasiSummary,
    ) {}
    public function definitions(): array
    {
        return [
            $this->tool('get_statistics', 'Statistik makro: total paket, total pagu, rata-rata progres, jumlah tiket. Pakai untuk pertanyaan "berapa/total/jumlah/ringkasan/statistik". Contoh: "berapa total pekerjaan tahun 2025" -> {tahun: 2025}.', [
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran, mis. 2025. Wajib diisi bila user menyebut tahun atau maksud tahun berjalan.'],
                'kecamatan' => ['type' => 'string', 'description' => 'Filter nama kecamatan, mis. "Cibeber". Kosongkan bila tidak disebut.'],
            ]),
            $this->tool('search_projects', 'Langkah 1 untuk cari paket: mengembalikan daftar paket dengan ID. Selalu panggil ini dulu sebelum get_project_details bila hanya tahu nama paket. Contoh: "cari paket SPAM Cibeber" -> {keyword: "SPAM Cibeber"}. Untuk paket batal: {status: "canceled"}.', [
                'keyword' => ['type' => 'string', 'description' => 'Kata kunci nama paket, desa, atau kecamatan. Gunakan kata paling spesifik dari pertanyaan user.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran bila disebut user, mis. 2025.'],
                'status' => ['type' => 'string', 'description' => 'active (berjalan) atau canceled (batal). Isi "canceled" bila user tanya paket batal/dibatalkan. Default semua status.'],
            ]),
            $this->tool('get_project_details', 'Langkah 2: detail lengkap satu paket (kontrak, progres, tiket, output, penerima). Hanya bisa dipanggil bila ID paket sudah diketahui dari search_projects. Jangan menebak ID.', [
                'id' => ['type' => 'integer', 'description' => 'ID paket persis dari hasil search_projects.'],
            ], ['id']),
            $this->tool('search_projects_by_progress', 'Cari paket berdasarkan kondisi progres fisik. Pakai untuk "belum 100%/belum selesai/deviasi/progres rendah/nol persen".', [
                'kondisi' => ['type' => 'string', 'description' => 'incomplete (fisik < 100), not_started (0%), behind (deviasi negatif), low_50 (di bawah 50%).'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran bila disebut user.'],
                'kecamatan' => ['type' => 'string', 'description' => 'Filter nama kecamatan bila disebut.'],
            ]),
            $this->tool('search_contracts', 'Cari kontrak/SPK berdasarkan nama paket, nomor SPK, atau nama penyedia. Contoh: "kontrak PT Maju" -> {keyword: "Maju"}.', [
                'keyword' => ['type' => 'string', 'description' => 'Nama paket, nomor SPK, atau nama penyedia.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran bila disebut user.'],
            ]),
            $this->tool('get_contractor_info', 'Profil penyedia + 5 histori pekerjaan terakhir. Cocok untuk "siapa/berapa proyek PT X". Bila nama tidak persis, cari dulu via search_contracts.', [
                'nama' => ['type' => 'string', 'description' => 'Nama penyedia, boleh sebagian, mis. "Maju".'],
            ], ['nama']),
            $this->tool('search_tickets', 'Cari tiket/laporan/keluhan. Pakai untuk "tiket/laporan/keluhan/masalah". Filter status bila user menyebut "terbuka/menunggu/selesai".', [
                'keyword' => ['type' => 'string', 'description' => 'Kata kunci subjek tiket.'],
                'status' => ['type' => 'string', 'description' => 'open (terbuka), pending (menunggu), atau closed (selesai).'],
                'kategori' => ['type' => 'string', 'description' => 'bug, request, lapangan, document, atau other.'],
            ]),
            $this->tool('search_photos', 'Cari dokumentasi foto. Pakai untuk "foto/dokumentasi/gambar".', [
                'keyword' => ['type' => 'string', 'description' => 'Nama paket, keterangan foto, atau nama komponen.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran bila disebut user.'],
            ]),
            $this->tool('search_outputs', 'Cari output/komponen/volume pekerjaan. Pakai untuk "output/komponen/volume/terpasang".', [
                'keyword' => ['type' => 'string', 'description' => 'Nama komponen (mis. "pipa", "reservoir") atau nama paket.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran bila disebut user.'],
            ]),
            $this->tool('search_recipients', 'Cari penerima manfaat. Pakai untuk "penerima/jiwa/KK/manfaat".', [
                'keyword' => ['type' => 'string', 'description' => 'Nama penerima atau nama paket.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran bila disebut user.'],
            ]),
            $this->tool('search_kegiatan', 'Cari kegiatan/program/sub-kegiatan + pagu + PPTK. Pakai untuk "kegiatan/program/sub kegiatan/pagu/DPA/sumber dana/PPTK". Contoh: "kegiatan air minum 2025" -> {keyword: "air minum", tahun: 2025}.', [
                'keyword' => ['type' => 'string', 'description' => 'Nama program, kegiatan, atau sub-kegiatan.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran bila disebut user.'],
                'sumber_dana' => ['type' => 'string', 'description' => 'APBD, APBN, DAU, DAK, DID, Bantuan Provinsi, DBH, SILPA, PAD, dst.'],
            ]),
            $this->tool('get_progress_trend', 'Tren progres fisik & keuangan satu paket dari histori estimasi (untuk chart line). Wajib ID dari search_projects dulu. Contoh jawaban: sertakan blok chart JSON line dari data bulanan.', [
                'id' => ['type' => 'integer', 'description' => 'ID paket persis dari hasil search_projects.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran; default tahun berjalan bila kosong.'],
            ], ['id']),
            $this->tool('get_konsolidasi', 'Grup konsolidasi: semua paket yang share kontrak yang sama (pivot kontrak_pekerjaan + legacy id_pekerjaan). Pakai untuk "konsolidasi/satu kontrak/gabungan paket". Wajib ID paket dari search_projects dulu.', [
                'id' => ['type' => 'integer', 'description' => 'ID paket persis dari hasil search_projects.'],
            ], ['id']),
            $this->tool('search_by_tags', 'Cari paket berdasarkan tag/label. Pakai untuk "tag/label/kategori paket". Tanpa argumen = daftar semua tag + jumlah paket.', [
                'tag' => ['type' => 'string', 'description' => 'Nama atau slug tag. Kosongkan untuk daftar semua tag.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran bila disebut user.'],
            ]),
            $this->tool('search_addendums', 'Cari addendum kontrak (perubahan nilai/waktu). Pakai untuk "addendum/perubahan kontrak/perpanjangan waktu/tambah kurang".', [
                'keyword' => ['type' => 'string', 'description' => 'Nomor addendum, nama paket, atau nomor SPK.'],
                'status' => ['type' => 'string', 'description' => 'Filter status addendum bila disebut user.'],
            ]),
            $this->tool('get_wilayah_summary', 'Agregat per kecamatan: jumlah desa, penduduk, KK, total paket + pagu + rata-rata progres. Pakai untuk "per kecamatan/sebaran wilayah/cakupan". Tanpa argumen = semua kecamatan.', [
                'kecamatan' => ['type' => 'string', 'description' => 'Nama kecamatan spesifik; kosongkan untuk semua.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran paket; default tahun berjalan.'],
            ]),
            $this->tool('search_usulan', 'Cari usulan/surat masuk kegiatan. Pakai untuk "usulan/surat masuk/permohonan/proposal".', [
                'keyword' => ['type' => 'string', 'description' => 'Perihal, ringkasan, nama pengusul, atau nomor surat.'],
                'kecamatan' => ['type' => 'string', 'description' => 'Filter nama kecamatan bila disebut.'],
            ]),
            $this->tool('search_spm_sanitasi', 'Cari infrastruktur sanitasi SPM (IPAL, TPS, truk tinja). Pakai untuk "sanitasi/IPAL/septik/truk tinja/SPM sanitasi".', [
                'keyword' => ['type' => 'string', 'description' => 'Nama infrastruktur, desa, atau alamat.'],
                'jenis' => ['type' => 'string', 'description' => 'Filter jenis infrastruktur bila disebut.'],
                'kecamatan' => ['type' => 'string', 'description' => 'Filter nama kecamatan bila disebut.'],
            ]),
            $this->tool('search_events', 'Cari agenda/kalender kegiatan. Pakai untuk "agenda/jadwal/event/kalender/kegiatan minggu ini".', [
                'keyword' => ['type' => 'string', 'description' => 'Judul, lokasi, atau deskripsi agenda.'],
                'kategori' => ['type' => 'string', 'description' => 'Filter kategori bila disebut.'],
                'upcoming' => ['type' => 'boolean', 'description' => 'True = hanya agenda mendatang/berjalan.'],
            ]),
            $this->tool('get_pengawas_info', 'Profil pengawas/pendamping + paket yang ditangani. Pakai untuk "pengawas/pendamping siapa/siapa pengawas paket X".', [
                'nama' => ['type' => 'string', 'description' => 'Nama pengawas, boleh sebagian.'],
            ], ['nama']),
            $this->tool('get_pengawas_kpi', 'Skor KPI pengawas (foto, penerima, output, fisik, PHO, progres) + breakdown per paket. Pakai untuk "KPI/kinerja/nilai/skor pengawas".', [
                'nama' => ['type' => 'string', 'description' => 'Nama pengawas/user, boleh sebagian. Kosongkan untuk peringkat semua.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran; default berjalan.'],
            ]),
            $this->tool('search_berkas', 'Cari arsip dokumen per paket. Pakai untuk "berkas/dokumen/arsip/file paket".', [
                'keyword' => ['type' => 'string', 'description' => 'Jenis dokumen atau nama paket.'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran bila disebut user.'],
            ]),
            $this->tool('get_ticket_details', 'Detail satu tiket + riwayat komentar. Wajib ID dari search_tickets dulu.', [
                'id' => ['type' => 'integer', 'description' => 'ID tiket persis dari hasil search_tickets.'],
            ], ['id']),
        ];
    }

    public function execute(string $name, array $args): array
    {
        return match ($name) {
            'get_statistics' => $this->getStatistics($args),
            'search_projects' => $this->searchProjects($args),
            'search_projects_by_progress' => $this->searchProjectsByProgress($args),
            'get_project_details' => $this->getProjectDetails($args),
            'search_contracts' => $this->searchContracts($args),
            'get_contractor_info' => $this->getContractorInfo($args),
            'search_tickets' => $this->searchTickets($args),
            'search_photos' => $this->searchPhotos($args),
            'search_outputs' => $this->searchOutputs($args),
            'search_recipients' => $this->searchRecipients($args),
            'search_kegiatan' => $this->searchKegiatan($args),
            'get_progress_trend' => $this->getProgressTrend($args),
            'get_konsolidasi' => $this->getKonsolidasi($args),
            'search_by_tags' => $this->searchByTags($args),
            'search_addendums' => $this->searchAddendums($args),
            'get_wilayah_summary' => $this->getWilayahSummary($args),
            'search_usulan' => $this->searchUsulan($args),
            'search_spm_sanitasi' => $this->searchSpmSanitasi($args),
            'search_events' => $this->searchEvents($args),
            'get_pengawas_info' => $this->getPengawasInfo($args),
            'get_pengawas_kpi' => $this->getPengawasKpi($args),
            'search_berkas' => $this->searchBerkas($args),
            'get_ticket_details' => $this->getTicketDetails($args),
            default => ['error' => 'Tool tidak ditemukan.'],
        };
    }

    private function tool(string $name, string $description, array $properties, array $required = []): array
    {
        return [
            'type' => 'function',
            'function' => [
                'name' => $name,
                'description' => $description,
                'parameters' => [
                    'type' => 'object',
                    'properties' => $properties,
                    'required' => $required,
                ],
            ],
        ];
    }

    private function baseProjectQuery(array $args = [])
    {
        $query = Pekerjaan::byUserRole();

        if (!empty($args['tahun'])) {
            $query->whereHas('kegiatan', fn($q) => $q->where('tahun_anggaran', $args['tahun']));
        }

        if (!empty($args['kecamatan'])) {
            $query->whereHas('kecamatan', fn($q) => $q->where('n_kec', 'LIKE', "%{$args['kecamatan']}%"));
        }

        if (!empty($args['status']) && in_array($args['status'], ['active', 'canceled'], true)) {
            $query->where('status', $args['status']);
        }

        return $query;
    }

    /** Tahun acuan = filter user, fallback tahun anggaran berjalan. Sama seperti Rekap. */
    private function resolveTahun(array $args): int
    {
        if (!empty($args['tahun'])) {
            return (int) $args['tahun'];
        }

        return (int) (AppSetting::getValue('tahun_anggaran') ?? date('Y'));
    }

    /** Ringkasan estimasi satu paket — sumber sama dengan Rekap Progress. */
    private function estimasiOf($project, int $tahun): array
    {
        return $this->estimasiSummary->summarize($project->progressEstimasiHistory ?? collect(), $tahun);
    }

    private function getStatistics(array $args): array
    {
        $tahun = $this->resolveTahun($args);
        $query = $this->baseProjectQuery($args);
        $ids = (clone $query)->pluck('id');
        $projects = (clone $query)->select('id', 'pagu')->with('progressEstimasiHistory')->get();

        $progressValues = $projects->map(fn($p) => $this->estimasiOf($p, $tahun)['fisik_realisasi'] ?? null)->filter(fn($v) => $v !== null);
        $averageProgress = $progressValues->count() > 0 ? round($progressValues->avg(), 2) : 0;

        return [
            'total_pekerjaan' => $projects->count(),
            'total_pagu' => (float) $projects->sum('pagu'),
            'formatted_total_pagu' => 'Rp ' . number_format($projects->sum('pagu'), 0, ',', '.'),
            'average_progress_percent' => $averageProgress,
            'total_tiket' => Tiket::whereIn('pekerjaan_id', $ids)->count(),
            'open_tiket' => Tiket::whereIn('pekerjaan_id', $ids)->where('status', 'open')->count(),
            'tahun' => $args['tahun'] ?? 'semua',
            'kecamatan' => $args['kecamatan'] ?? 'semua',
        ];
    }

    private function searchProjects(array $args): array
    {
        $tahun = $this->resolveTahun($args);
        $query = $this->baseProjectQuery($args)->with(['kecamatan', 'desa', 'kegiatan', 'progressEstimasiHistory']);

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('nama_paket', 'LIKE', "%{$keyword}%")
                    ->orWhereHas('kecamatan', fn($sub) => $sub->where('n_kec', 'LIKE', "%{$keyword}%"))
                    ->orWhereHas('desa', fn($sub) => $sub->where('n_desa', 'LIKE', "%{$keyword}%"));
            });
        }

        return [
            'results' => $query->limit(15)->get()->map(fn($p) => [
                'id' => $p->id,
                'nama_paket' => $p->nama_paket,
                'lokasi' => ($p->desa->n_desa ?? '-') . ', ' . ($p->kecamatan->n_kec ?? '-'),
                'tahun' => $p->kegiatan->tahun_anggaran ?? null,
                'pagu' => (float) $p->pagu,
                'status' => $p->status ?? 'active',
                'progress_fisik' => $this->estimasiOf($p, (int) ($p->kegiatan->tahun_anggaran ?? $tahun))['fisik_realisasi'],
                'progress_keuangan' => $this->estimasiOf($p, (int) ($p->kegiatan->tahun_anggaran ?? $tahun))['keuangan_realisasi'],
            ]),
        ];
    }

    private function searchProjectsByProgress(array $args): array
    {
        $tahun = $this->resolveTahun($args);
        $kondisi = $args['kondisi'] ?? 'incomplete';

        $pakets = $this->baseProjectQuery($args)
            ->with(['kecamatan', 'desa', 'kegiatan', 'progressEstimasiHistory'])
            ->whereHas('progressEstimasiHistory', fn($q) => $q->where('tahun_anggaran', $tahun))
            ->limit(200)
            ->get()
            ->map(function ($p) use ($tahun) {
                $t = (int) ($p->kegiatan->tahun_anggaran ?? $tahun);
                $e = $this->estimasiOf($p, $t);

                return [
                    'id' => $p->id,
                    'nama_paket' => $p->nama_paket,
                    'lokasi' => ($p->desa->n_desa ?? '-') . ', ' . ($p->kecamatan->n_kec ?? '-'),
                    'tahun' => $p->kegiatan->tahun_anggaran ?? null,
                    'pagu' => (float) $p->pagu,
                    'progress_fisik' => $e['fisik_realisasi'],
                    'deviasi_fisik' => $e['fisik_deviasi'],
                ];
            })
            ->filter(function ($p) use ($kondisi) {
                $fisik = $p['progress_fisik'];
                if ($fisik === null) {
                    return $kondisi === 'incomplete';
                }

                return match ($kondisi) {
                    'not_started' => $fisik <= 0,
                    'behind' => ($p['deviasi_fisik'] ?? 0) < 0,
                    'low_50' => $fisik < 50,
                    default => $fisik < 100,
                };
            })
            ->sortBy('progress_fisik')
            ->take(15)
            ->values();

        return ['results' => $pakets, 'kondisi' => $kondisi, 'tahun' => $tahun];
    }

    private function getProjectDetails(array $args): array
    {
        $project = Pekerjaan::byUserRole()
            ->with([
                'kontrak.penyedia', 'kontrak.addendums', 'kontrak.registers.type',
                'progressEstimasiHistory', 'kecamatan', 'desa', 'kegiatan',
                'tiket', 'output', 'penerima', 'berkas', 'beritaAcara',
                'pengawas', 'pendamping',
            ])
            ->find($args['id'] ?? null);

        if (!$project) {
            return ['error' => 'Paket tidak ditemukan.'];
        }

        $tahun = (int) ($project->kegiatan->tahun_anggaran ?? $this->resolveTahun($args));
        $estimasi = $this->estimasiOf($project, $tahun);

        $addendums = $project->kontrak->flatMap(fn($k) => $k->addendums ?? collect())->values();

        return [
            'id' => $project->id,
            'nama' => $project->nama_paket,
            'tahun' => $project->kegiatan->tahun_anggaran ?? null,
            'lokasi' => ($project->desa->n_desa ?? '-') . ', ' . ($project->kecamatan->n_kec ?? '-'),
            'pagu' => (float) $project->pagu,
            'status' => $project->status ?? 'active',
            'progress_fisik' => $estimasi['fisik_realisasi'],
            'progress_keuangan' => $estimasi['keuangan_realisasi'],
            'deviasi_fisik' => $estimasi['fisik_deviasi'],
            'deviasi_keuangan' => $estimasi['keuangan_deviasi'],
            'kontrak' => $project->kontrak->map(fn($k) => [
                'nilai' => (float) $k->nilai_kontrak,
                'nilai_berjalan' => $k->nilaiKontrakBerjalan(),
                'penyedia' => $k->penyedia->nama ?? 'N/A',
                'spk' => $k->spk,
                'tgl_selesai' => $k->tglSelesaiBerjalan(),
            ]),
            'addendums' => $addendums->map(fn($a) => [
                'nomor' => $a->nomor_addendum,
                'jenis' => $a->jenis_addendum,
                'nilai_sebelum' => (float) $a->nilai_kontrak_sebelum,
                'nilai_sesudah' => (float) $a->nilai_kontrak_sesudah,
                'status' => $a->status,
            ]),
            'dokumen_kontrak' => $project->kontrak->flatMap(fn($k) => $k->registers ?? collect())->map(fn($r) => [
                'jenis' => $r->type->nama ?? $r->attachment_type,
                'nomor' => $r->nomor,
                'tanggal' => $r->tanggal?->format('Y-m-d'),
            ])->values(),
            'berita_acara' => $project->beritaAcara ? ($project->beritaAcara->data ?? []) : null,
            'berkas' => $project->berkas->map(fn($b) => $b->jenis_dokumen)->unique()->values(),
            'pengawas' => $project->pengawas->nama ?? null,
            'pendamping' => $project->pendamping->nama ?? null,
            'jumlah_output' => $project->output->count(),
            'jumlah_penerima' => $project->penerima->count(),
            'jumlah_jiwa' => (int) $project->penerima->sum('jumlah_jiwa'),
            'tiket' => $project->tiket->map(fn($t) => [
                'subjek' => $t->subjek,
                'status' => $t->status,
                'prioritas' => $t->prioritas,
            ]),
        ];
    }

    private function searchContracts(array $args): array
    {
        $query = Kontrak::with(['pekerjaan.kegiatan', 'penyedia'])
            ->whereHas('pekerjaan', fn($q) => $q->byUserRole());

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('spk', 'LIKE', "%{$keyword}%")
                    ->orWhereHas('pekerjaan', fn($sub) => $sub->where('nama_paket', 'LIKE', "%{$keyword}%"))
                    ->orWhereHas('penyedia', fn($sub) => $sub->where('nama', 'LIKE', "%{$keyword}%"));
            });
        }

        if (!empty($args['tahun'])) {
            $query->whereHas('pekerjaan.kegiatan', fn($q) => $q->where('tahun_anggaran', $args['tahun']));
        }

        return [
            'results' => $query->latest()->limit(15)->get()->map(fn($k) => [
                'id' => $k->id,
                'paket' => $k->pekerjaan->nama_paket ?? 'N/A',
                'penyedia' => $k->penyedia->nama ?? 'N/A',
                'spk' => $k->spk,
                'nilai_kontrak' => (float) $k->nilai_kontrak,
                'tahun' => $k->pekerjaan->kegiatan->tahun_anggaran ?? null,
            ]),
        ];
    }

    private function getContractorInfo(array $args): array
    {
        $provider = Penyedia::where('nama', 'LIKE', '%' . ($args['nama'] ?? '') . '%')->first();
        if (!$provider) {
            return ['error' => 'Penyedia tidak ditemukan.'];
        }

        $contracts = Kontrak::where('id_penyedia', $provider->id)
            ->whereHas('pekerjaan', fn($q) => $q->byUserRole())
            ->with('pekerjaan.kegiatan')
            ->latest()
            ->limit(5)
            ->get();

        return [
            'nama' => $provider->nama,
            'direktur' => $provider->direktur,
            'histori_pekerjaan' => $contracts->map(fn($k) => [
                'paket' => $k->pekerjaan->nama_paket ?? 'N/A',
                'nilai' => (float) $k->nilai_kontrak,
                'tahun' => $k->pekerjaan->kegiatan->tahun_anggaran ?? null,
            ]),
        ];
    }

    private function searchTickets(array $args): array
    {
        // ponytail: tiket yatim (pekerjaan_id NULL, onDelete set null) ikut tampil.
        $query = Tiket::with('pekerjaan')
            ->where(fn($q) => $q->whereNull('pekerjaan_id')
                ->orWhereHas('pekerjaan', fn($sub) => $sub->byUserRole()));

        if (!empty($args['keyword'])) {
            $query->where('subjek', 'LIKE', "%{$args['keyword']}%");
        }
        if (!empty($args['status'])) {
            $query->where('status', $args['status']);
        }
        if (!empty($args['kategori'])) {
            $query->where('kategori', $args['kategori']);
        }

        return [
            'results' => $query->latest()->limit(15)->get()->map(fn($t) => [
                'id' => $t->id,
                'subjek' => $t->subjek,
                'status' => $t->status,
                'prioritas' => $t->prioritas,
                'kategori' => $t->kategori,
                'paket' => $t->pekerjaan->nama_paket ?? null,
            ]),
        ];
    }

    private function searchPhotos(array $args): array
    {
        $query = Foto::with(['pekerjaan.kegiatan', 'komponen'])
            ->whereHas('pekerjaan', fn($q) => $q->byUserRole());

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('keterangan', 'LIKE', "%{$keyword}%")
                    ->orWhereHas('pekerjaan', fn($sub) => $sub->where('nama_paket', 'LIKE', "%{$keyword}%"))
                    ->orWhereHas('komponen', fn($sub) => $sub->where('komponen', 'LIKE', "%{$keyword}%"));
            });
        }

        if (!empty($args['tahun'])) {
            $query->whereHas('pekerjaan.kegiatan', fn($q) => $q->where('tahun_anggaran', $args['tahun']));
        }

        return [
            'results' => $query->latest()->limit(15)->get()->map(fn($f) => [
                'id' => $f->id,
                'paket' => $f->pekerjaan->nama_paket ?? null,
                'komponen' => $f->komponen->komponen ?? null,
                'keterangan' => $f->keterangan,
                'foto_url' => $f->getFirstMediaUrl('foto/pekerjaan'),
                'thumb_url' => $f->getFirstMediaUrl('foto/pekerjaan', 'thumb') ?: $f->getFirstMediaUrl('foto/pekerjaan'),
                'validasi_koordinat' => (bool) $f->validasi_koordinat,
                'tahun' => $f->pekerjaan->kegiatan->tahun_anggaran ?? null,
            ]),
        ];
    }

    private function searchOutputs(array $args): array
    {
        $query = Output::with('pekerjaan.kegiatan')
            ->whereHas('pekerjaan', fn($q) => $q->byUserRole());

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('komponen', 'LIKE', "%{$keyword}%")
                    ->orWhereHas('pekerjaan', fn($sub) => $sub->where('nama_paket', 'LIKE', "%{$keyword}%"));
            });
        }

        if (!empty($args['tahun'])) {
            $query->whereHas('pekerjaan.kegiatan', fn($q) => $q->where('tahun_anggaran', $args['tahun']));
        }

        return [
            'results' => $query->latest()->limit(15)->get()->map(fn($o) => [
                'id' => $o->id,
                'komponen' => $o->komponen,
                'satuan' => $o->satuan,
                'volume' => (float) $o->volume,
                'paket' => $o->pekerjaan->nama_paket ?? null,
                'tahun' => $o->pekerjaan->kegiatan->tahun_anggaran ?? null,
            ]),
        ];
    }

    private function searchRecipients(array $args): array
    {
        $query = Penerima::with('pekerjaan.kegiatan')
            ->whereHas('pekerjaan', fn($q) => $q->byUserRole());

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('nama', 'LIKE', "%{$keyword}%")
                    ->orWhereHas('pekerjaan', fn($sub) => $sub->where('nama_paket', 'LIKE', "%{$keyword}%"));
            });
        }

        if (!empty($args['tahun'])) {
            $query->whereHas('pekerjaan.kegiatan', fn($q) => $q->where('tahun_anggaran', $args['tahun']));
        }

        return [
            'results' => $query->latest()->limit(15)->get()->map(fn($p) => [
                'id' => $p->id,
                'nama' => $p->nama,
                'jumlah_jiwa' => (int) $p->jumlah_jiwa,
                'is_komunal' => (bool) $p->is_komunal,
                'paket' => $p->pekerjaan->nama_paket ?? null,
                'tahun' => $p->pekerjaan->kegiatan->tahun_anggaran ?? null,
            ]),
        ];
    }

    private function searchKegiatan(array $args): array
    {
        $query = Kegiatan::query();

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('nama_program', 'LIKE', "%{$keyword}%")
                    ->orWhere('nama_kegiatan', 'LIKE', "%{$keyword}%")
                    ->orWhere('nama_sub_kegiatan', 'LIKE', "%{$keyword}%");
            });
        }

        if (!empty($args['tahun'])) {
            $query->where('tahun_anggaran', $args['tahun']);
        }

        if (!empty($args['sumber_dana'])) {
            $query->where('sumber_dana', 'LIKE', "%{$args['sumber_dana']}%");
        }

        $rows = $query->latest('tahun_anggaran')->limit(15)->get();

        return [
            'results' => $rows->map(fn($k) => [
                'id' => $k->id,
                'program' => $k->nama_program,
                'kegiatan' => $k->nama_kegiatan,
                'sub_kegiatan' => $k->nama_sub_kegiatan,
                'tahun' => $k->tahun_anggaran,
                'sumber_dana' => $k->sumber_dana,
                'pagu' => (float) $k->pagu,
                'pptk' => $k->nama_pptk,
            ]),
            'total_pagu' => (float) $rows->sum('pagu'),
        ];
    }

    private function getProgressTrend(array $args): array
    {
        $project = Pekerjaan::byUserRole()->find($args['id'] ?? null);
        if (!$project) {
            return ['error' => 'Paket tidak ditemukan.'];
        }

        $tahun = (int) ($args['tahun'] ?? $project->kegiatan->tahun_anggaran ?? $this->resolveTahun($args));

        $histories = PekerjaanProgressEstimasiHistory::where('pekerjaan_id', $project->id)
            ->where('tahun_anggaran', $tahun)
            ->orderBy('tanggal')
            ->get();

        $monthly = [];
        foreach ($histories as $h) {
            $key = $h->tanggal?->format('Y-m') ?? (string) $tahun;
            $monthly[$key] ??= ['bulan' => $key, 'fisik' => null, 'keuangan' => null];
            if ($h->tipe === 'realisasi') {
                $monthly[$key][$h->jenis] = (float) $h->persen;
            }
        }

        return [
            'paket' => $project->nama_paket,
            'tahun' => $tahun,
            'ringkasan' => $this->estimasiOf($project, $tahun),
            'tren_bulanan' => array_values($monthly),
        ];
    }

    private function getKonsolidasi(array $args): array
    {
        $project = Pekerjaan::byUserRole()
            ->with(['kontrak.penyedia', 'kontrakLegacy.penyedia', 'kegiatan'])
            ->find($args['id'] ?? null);
        if (!$project) {
            return ['error' => 'Paket tidak ditemukan.'];
        }

        // Kumpulkan kontrak via pivot konsolidasi + legacy id_pekerjaan.
        $kontraks = $project->kontrak->concat($project->kontrakLegacy)->unique('id')->values();
        if ($kontraks->isEmpty()) {
            return ['paket' => $project->nama_paket, 'konsolidasi' => false, 'pesan' => 'Paket belum punya kontrak.'];
        }

        $grup = [];
        foreach ($kontraks as $k) {
            // Pivot konsolidasi + legacy id_pekerjaan, dibatasi role user.
            $viaPivot = $k->pekerjaans()->byUserRole()->pluck('tbl_pekerjaan.id');
            $viaLegacy = $k->pekerjaan && Pekerjaan::byUserRole()->where('tbl_pekerjaan.id', $k->pekerjaan->id)->exists()
                ? collect([$k->pekerjaan->id])
                : collect();
            $pakets = Pekerjaan::byUserRole()
                ->whereIn('tbl_pekerjaan.id', $viaPivot->concat($viaLegacy)->unique()->values())
                ->with('kegiatan')
                ->get();

            $grup[] = [
                'spk' => $k->spk,
                'penyedia' => $k->penyedia->nama ?? 'N/A',
                'nilai_kontrak' => (float) $k->nilai_kontrak,
                'paket' => $pakets->map(fn($p) => [
                    'id' => $p->id,
                    'nama_paket' => $p->nama_paket,
                    'tahun' => $p->kegiatan->tahun_anggaran ?? null,
                    'pagu' => (float) $p->pagu,
                ]),
                'total_pagu' => (float) $pakets->sum('pagu'),
            ];
        }

        return [
            'paket' => $project->nama_paket,
            'konsolidasi' => count($grup) > 0 && ($grup[0]['paket'] ?? collect())->count() > 1,
            'grup' => $grup,
        ];
    }

    private function searchByTags(array $args): array
    {
        if (empty($args['tag'])) {
            return [
                'tags' => Tag::withCount(['pekerjaan' => fn($q) => $q->byUserRole()])->get()->map(fn($t) => [
                    'nama' => $t->name,
                    'slug' => $t->slug,
                    'jumlah_paket' => $t->pekerjaan_count,
                ]),
            ];
        }

        $query = Pekerjaan::byUserRole()
            ->whereHas('tags', fn($q) => $q->where('name', 'LIKE', "%{$args['tag']}%")->orWhere('slug', 'LIKE', "%{$args['tag']}%"))
            ->with(['tags', 'kecamatan', 'desa', 'kegiatan']);

        if (!empty($args['tahun'])) {
            $query->whereHas('kegiatan', fn($q) => $q->where('tahun_anggaran', $args['tahun']));
        }

        return [
            'results' => $query->limit(15)->get()->map(fn($p) => [
                'id' => $p->id,
                'nama_paket' => $p->nama_paket,
                'tags' => $p->tags->pluck('name'),
                'lokasi' => ($p->desa->n_desa ?? '-') . ', ' . ($p->kecamatan->n_kec ?? '-'),
                'tahun' => $p->kegiatan->tahun_anggaran ?? null,
            ]),
        ];
    }

    private function searchAddendums(array $args): array
    {
        $query = KontrakAddendum::with(['kontrak.pekerjaan', 'kontrak.penyedia'])
            ->whereHas('kontrak.pekerjaan', fn($q) => $q->byUserRole());

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('nomor_addendum', 'LIKE', "%{$keyword}%")
                    ->orWhereHas('kontrak', fn($sub) => $sub->where('spk', 'LIKE', "%{$keyword}%"))
                    ->orWhereHas('kontrak.pekerjaan', fn($sub) => $sub->where('nama_paket', 'LIKE', "%{$keyword}%"));
            });
        }

        if (!empty($args['status'])) {
            $query->where('status', $args['status']);
        }

        return [
            'results' => $query->latest()->limit(15)->get()->map(fn($a) => [
                'id' => $a->id,
                'nomor' => $a->nomor_addendum,
                'addendum_ke' => $a->addendum_ke,
                'paket' => $a->kontrak->pekerjaan->nama_paket ?? 'N/A',
                'penyedia' => $a->kontrak->penyedia->nama ?? 'N/A',
                'jenis' => $a->jenis_addendum,
                'nilai_sebelum' => (float) $a->nilai_kontrak_sebelum,
                'nilai_sesudah' => (float) $a->nilai_kontrak_sesudah,
                'selesai_sebelum' => $a->tgl_selesai_sebelum?->format('Y-m-d'),
                'selesai_sesudah' => $a->tgl_selesai_sesudah?->format('Y-m-d'),
                'status' => $a->status,
            ]),
        ];
    }

    private function getWilayahSummary(array $args): array
    {
        $tahun = (int) ($args['tahun'] ?? $this->resolveTahun($args));

        $query = Kecamatan::realWilayah()->withCount('desa');
        if (!empty($args['kecamatan'])) {
            $query->where('n_kec', 'LIKE', "%{$args['kecamatan']}%");
        }

        return [
            'tahun' => $tahun,
            'results' => $query->get()->map(function ($kec) use ($tahun) {
                $desaIds = Desa::where('kecamatan_id', $kec->id)->pluck('id');
                $paketQuery = Pekerjaan::byUserRole()->where('kecamatan_id', $kec->id);
                if ($tahun) {
                    $paketQuery->whereHas('kegiatan', fn($q) => $q->where('tahun_anggaran', $tahun));
                }
                $pakets = $paketQuery->with('progressEstimasiHistory')->get();
                $progress = $pakets->map(fn($p) => $this->estimasiOf($p, $tahun)['fisik_realisasi'] ?? null)
                    ->filter(fn($v) => $v !== null);

                return [
                    'kecamatan' => $kec->n_kec,
                    'jumlah_desa' => $kec->desa_count,
                    'jumlah_penduduk' => (int) Desa::where('kecamatan_id', $kec->id)->sum('jumlah_penduduk'),
                    'jumlah_kk' => (int) Desa::where('kecamatan_id', $kec->id)->sum('jumlah_kk'),
                    'total_paket' => $pakets->count(),
                    'total_pagu' => (float) $pakets->sum('pagu'),
                    'rata_progres' => $progress->count() > 0 ? round($progress->avg(), 2) : 0,
                    'total_jiwa_terlayani' => (int) Penerima::whereIn('pekerjaan_id', $pakets->pluck('id'))->sum('jumlah_jiwa'),
                ];
            }),
        ];
    }

    private function searchUsulan(array $args): array
    {
        $query = UsulanKegiatan::with(['kecamatan', 'desa']);

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('perihal', 'LIKE', "%{$keyword}%")
                    ->orWhere('ringkasan', 'LIKE', "%{$keyword}%")
                    ->orWhere('nama_pengusul', 'LIKE', "%{$keyword}%")
                    ->orWhere('nomor_surat_masuk', 'LIKE', "%{$keyword}%");
            });
        }

        if (!empty($args['kecamatan'])) {
            $query->whereHas('kecamatan', fn($q) => $q->where('n_kec', 'LIKE', "%{$args['kecamatan']}%"));
        }

        return [
            'results' => $query->latest('tanggal_surat_masuk')->limit(15)->get()->map(fn($u) => [
                'id' => $u->id,
                'perihal' => $u->perihal,
                'pengusul' => $u->nama_pengusul,
                'wilayah' => ($u->desa->n_desa ?? '-') . ', ' . ($u->kecamatan->n_kec ?? '-'),
                'nomor_surat' => $u->nomor_surat_masuk,
                'tanggal_surat' => $u->tanggal_surat?->format('Y-m-d'),
                'tanggal_masuk' => $u->tanggal_surat_masuk?->format('Y-m-d'),
                'ringkasan' => $u->ringkasan,
            ]),
        ];
    }

    private function searchSpmSanitasi(array $args): array
    {
        $query = SpmSanitasi::with(['desa.kecamatan']);

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('nama_infrastruktur', 'LIKE', "%{$keyword}%")
                    ->orWhere('alamat_lengkap', 'LIKE', "%{$keyword}%")
                    ->orWhereHas('desa', fn($sub) => $sub->where('n_desa', 'LIKE', "%{$keyword}%"));
            });
        }

        if (!empty($args['jenis'])) {
            $query->where('jenis', 'LIKE', "%{$args['jenis']}%");
        }

        if (!empty($args['kecamatan'])) {
            $query->whereHas('desa.kecamatan', fn($q) => $q->where('n_kec', 'LIKE', "%{$args['kecamatan']}%"));
        }

        return [
            'results' => $query->latest()->limit(15)->get()->map(fn($s) => [
                'id' => $s->id,
                'nama' => $s->nama_infrastruktur,
                'jenis' => $s->jenis,
                'lokasi' => ($s->desa->n_desa ?? '-') . ', ' . ($s->desa->kecamatan->n_kec ?? '-'),
                'pemanfaat_kk' => (int) $s->jumlah_pemanfaat_kk,
                'pemanfaat_jiwa' => (int) $s->jumlah_pemanfaat_jiwa,
                'keberfungsian' => $s->status_keberfungsian,
                'tahun_konstruksi' => $s->tahun_konstruksi,
                'pembiayaan_total' => (float) $s->pembiayaan_total,
            ]),
        ];
    }

    private function searchEvents(array $args): array
    {
        $query = Event::query();

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('title', 'LIKE', "%{$keyword}%")
                    ->orWhere('location', 'LIKE', "%{$keyword}%")
                    ->orWhere('description', 'LIKE', "%{$keyword}%");
            });
        }

        if (!empty($args['kategori'])) {
            $query->where('category', $args['kategori']);
        }

        if (!empty($args['upcoming'])) {
            $query->where('end', '>=', now());
        }

        return [
            'results' => $query->orderBy('start')->limit(15)->get()->map(fn($e) => [
                'id' => $e->id,
                'judul' => $e->title,
                'mulai' => $e->start?->format('Y-m-d H:i'),
                'selesai' => $e->end?->format('Y-m-d H:i'),
                'lokasi' => $e->location,
                'kategori' => $e->category,
            ]),
        ];
    }

    private function getPengawasInfo(array $args): array
    {
        $pengawas = Pengawas::where('nama', 'LIKE', '%' . ($args['nama'] ?? '') . '%')->first();
        if (!$pengawas) {
            return ['error' => 'Pengawas tidak ditemukan.'];
        }

        $pakets = Pekerjaan::byUserRole()
            ->where(fn($q) => $q->where('pengawas_id', $pengawas->id)->orWhere('pendamping_id', $pengawas->id))
            ->with('kegiatan')
            ->latest()
            ->limit(10)
            ->get();

        return [
            'nama' => $pengawas->nama,
            'nip' => $pengawas->nip,
            'jabatan' => $pengawas->jabatan,
            'telepon' => $pengawas->telepon,
            'paket_ditangani' => $pakets->map(fn($p) => [
                'id' => $p->id,
                'nama_paket' => $p->nama_paket,
                'peran' => $p->pengawas_id === $pengawas->id ? 'pengawas' : 'pendamping',
                'tahun' => $p->kegiatan->tahun_anggaran ?? null,
            ]),
        ];
    }

    private function getPengawasKpi(array $args): array
    {
        // ponytail: reuse PuspenPengawasKpiController agar rumus skor tunggal.
        // Pisah ke service bila tool butuh varian agregat lain.
        $controller = app(\App\Http\Controllers\PuspenPengawasKpiController::class);
        $tahun = (int) ($args['tahun'] ?? $this->resolveTahun($args));

        if (!empty($args['nama'])) {
            $user = User::where('name', 'LIKE', '%' . $args['nama'] . '%')
                ->whereHas('roles', fn($q) => $q->whereIn('name', ['pengawas', 'konsultan_pengawas']))
                ->first();
            if (!$user) {
                return ['error' => 'Pengawas tidak ditemukan.'];
            }

            $response = $controller->show(Request::create('/', 'GET', ['tahun' => $tahun]), $user->id);
            $data = $response->getData(true);

            return [
                'nama' => $data['user']['nama'] ?? $user->name,
                'tahun' => $tahun,
                'ringkasan' => $data['summary'] ?? [],
                'paket' => array_slice(array_map(fn($p) => [
                    'nama_paket' => $p['nama_paket'] ?? null,
                    'skor' => $p['score'] ?? null,
                    'progres' => $p['progress_realisasi'] ?? null,
                    'catatan' => $p['catatan'] ?? null,
                ], $data['pekerjaan'] ?? []), 0, 10),
            ];
        }

        $response = $controller->index(Request::create('/', 'GET', [
            'tahun' => $tahun,
            'per_page' => 15,
        ]));
        $data = $response->getData(true);

        return [
            'tahun' => $tahun,
            'ringkasan' => $data['summary'] ?? [],
            'peringkat' => array_map(fn($r) => [
                'rank' => $r['rank'] ?? null,
                'nama' => $r['nama'] ?? null,
                'skor_rata' => $r['score_per_pekerjaan'] ?? null,
                'skor_total' => $r['score'] ?? null,
                'paket' => $r['pekerjaan_count'] ?? null,
                'paket_bagus' => $r['quality_packages'] ?? null,
            ], $data['data'] ?? []),
        ];
    }

    private function searchBerkas(array $args): array
    {
        $query = Berkas::with(['pekerjaan.kegiatan'])
            ->whereHas('pekerjaan', fn($q) => $q->byUserRole());

        if (!empty($args['keyword'])) {
            $keyword = $args['keyword'];
            $query->where(function ($q) use ($keyword) {
                $q->where('jenis_dokumen', 'LIKE', "%{$keyword}%")
                    ->orWhereHas('pekerjaan', fn($sub) => $sub->where('nama_paket', 'LIKE', "%{$keyword}%"));
            });
        }

        if (!empty($args['tahun'])) {
            $query->whereHas('pekerjaan.kegiatan', fn($q) => $q->where('tahun_anggaran', $args['tahun']));
        }

        return [
            'results' => $query->latest()->limit(15)->get()->map(fn($b) => [
                'id' => $b->id,
                'jenis_dokumen' => $b->jenis_dokumen,
                'paket' => $b->pekerjaan->nama_paket ?? null,
                'file_url' => $b->getFirstMediaUrl('berkas/dokumen'),
                'tahun' => $b->pekerjaan->kegiatan->tahun_anggaran ?? null,
            ]),
        ];
    }

    private function getTicketDetails(array $args): array
    {
        // ponytail: tiket yatim (pekerjaan_id NULL, 74/76 row) tetap ditampilkan.
        // Naikkan ke whereHas bila skema kelak mewajibkan pekerjaan_id NOT NULL.
        $tiket = Tiket::with(['pekerjaan', 'comments.user', 'user'])
            ->where(function ($q) {
                $q->whereNull('pekerjaan_id')
                    ->orWhereHas('pekerjaan', fn($sub) => $sub->byUserRole());
            })
            ->find($args['id'] ?? null);

        if (!$tiket) {
            return ['error' => 'Tiket tidak ditemukan.'];
        }

        return [
            'id' => $tiket->id,
            'subjek' => $tiket->subjek,
            'deskripsi' => $tiket->deskripsi,
            'status' => $tiket->status,
            'prioritas' => $tiket->prioritas,
            'kategori' => $tiket->kategori,
            'paket' => $tiket->pekerjaan->nama_paket ?? null,
            'pelapor' => $tiket->user->name ?? null,
            'catatan_admin' => $tiket->admin_notes,
            'komentar' => $tiket->comments->map(fn($c) => [
                'penulis' => $c->user->name ?? null,
                'pesan' => $c->message,
                'waktu' => $c->created_at?->format('Y-m-d H:i'),
            ]),
        ];
    }

}

