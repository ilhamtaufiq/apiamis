<?php

namespace App\Services;

use App\Models\Foto;
use App\Models\Kontrak;
use App\Models\Output;
use App\Models\Pekerjaan;
use App\Models\Penerima;
use App\Models\Penyedia;
use App\Models\Tiket;

class ChatDataToolService
{
    public function definitions(): array
    {
        return [
            $this->tool('get_statistics', 'Mendapatkan statistik makro pekerjaan, pagu, tiket, dan progres.', [
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran'],
                'kecamatan' => ['type' => 'string', 'description' => 'Nama kecamatan'],
            ]),
            $this->tool('search_projects', 'Mencari daftar paket pekerjaan.', [
                'keyword' => ['type' => 'string', 'description' => 'Kata kunci nama paket, desa, atau kecamatan'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran'],
            ]),
            $this->tool('get_project_details', 'Mendapatkan detail lengkap satu paket pekerjaan.', [
                'id' => ['type' => 'integer', 'description' => 'ID paket'],
            ], ['id']),
            $this->tool('search_contracts', 'Mencari kontrak berdasarkan paket atau penyedia.', [
                'keyword' => ['type' => 'string', 'description' => 'Nama paket, nomor SPK, atau penyedia'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran'],
            ]),
            $this->tool('get_contractor_info', 'Mendapatkan info penyedia dan histori pekerjaan terakhir.', [
                'nama' => ['type' => 'string', 'description' => 'Nama penyedia'],
            ], ['nama']),
            $this->tool('search_tickets', 'Mencari tiket/laporan berdasarkan subjek, status, atau kategori.', [
                'keyword' => ['type' => 'string', 'description' => 'Kata kunci subjek tiket'],
                'status' => ['type' => 'string', 'description' => 'open, pending, atau closed'],
                'kategori' => ['type' => 'string', 'description' => 'bug, request, lapangan, document, atau other'],
            ]),
            $this->tool('search_photos', 'Mencari dokumentasi foto pekerjaan.', [
                'keyword' => ['type' => 'string', 'description' => 'Nama paket, keterangan foto, atau komponen'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran'],
            ]),
            $this->tool('search_outputs', 'Mencari output/komponen pekerjaan.', [
                'keyword' => ['type' => 'string', 'description' => 'Nama komponen atau paket pekerjaan'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran'],
            ]),
            $this->tool('search_recipients', 'Mencari penerima manfaat berdasarkan nama atau paket pekerjaan.', [
                'keyword' => ['type' => 'string', 'description' => 'Nama penerima atau paket pekerjaan'],
                'tahun' => ['type' => 'integer', 'description' => 'Tahun anggaran'],
            ]),
        ];
    }

    public function execute(string $name, array $args): array
    {
        return match ($name) {
            'get_statistics' => $this->getStatistics($args),
            'search_projects' => $this->searchProjects($args),
            'get_project_details' => $this->getProjectDetails($args),
            'search_contracts' => $this->searchContracts($args),
            'get_contractor_info' => $this->getContractorInfo($args),
            'search_tickets' => $this->searchTickets($args),
            'search_photos' => $this->searchPhotos($args),
            'search_outputs' => $this->searchOutputs($args),
            'search_recipients' => $this->searchRecipients($args),
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

        return $query;
    }

    private function getStatistics(array $args): array
    {
        $query = $this->baseProjectQuery($args);
        $ids = (clone $query)->pluck('id');
        $projects = (clone $query)->select('id', 'pagu')->with('progress:pekerjaan_id,content')->get();

        $progressValues = $projects->map(fn($p) => $this->calculateProgressTotal($p->progress?->content ?? []));
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
        $query = $this->baseProjectQuery($args)->with(['kecamatan', 'desa', 'kegiatan', 'progress']);

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
                'progress_percent' => $this->calculateProgressTotal($p->progress?->content ?? []),
            ]),
        ];
    }

    private function getProjectDetails(array $args): array
    {
        $project = Pekerjaan::byUserRole()
            ->with(['kontrak.penyedia', 'progress', 'kecamatan', 'desa', 'kegiatan', 'tiket', 'output', 'penerima'])
            ->find($args['id'] ?? null);

        if (!$project) {
            return ['error' => 'Paket tidak ditemukan.'];
        }

        return [
            'id' => $project->id,
            'nama' => $project->nama_paket,
            'tahun' => $project->kegiatan->tahun_anggaran ?? null,
            'lokasi' => ($project->desa->n_desa ?? '-') . ', ' . ($project->kecamatan->n_kec ?? '-'),
            'pagu' => (float) $project->pagu,
            'progress_percent' => $this->calculateProgressTotal($project->progress?->content ?? []),
            'kontrak' => $project->kontrak->map(fn($k) => [
                'nilai' => (float) $k->nilai_kontrak,
                'penyedia' => $k->penyedia->nama ?? 'N/A',
                'spk' => $k->spk,
            ]),
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
        $query = Tiket::with('pekerjaan')
            ->whereHas('pekerjaan', fn($q) => $q->byUserRole());

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

    private function calculateProgressTotal(array $content): float
    {
        $items = $content['items'] ?? [];
        $progressTotal = 0;

        foreach ($items as $item) {
            $weight = (float) ($item['bobot'] ?? 0);
            $targetVolume = (float) ($item['target_volume'] ?? 0);
            $actual = 0;

            foreach (($item['weekly_data'] ?? []) as $weeklyData) {
                if (($weeklyData['realisasi'] ?? null) !== null) {
                    $actual += (float) $weeklyData['realisasi'];
                }
            }

            $progressPercent = $targetVolume > 0 ? ($actual / $targetVolume) * 100 : 0;
            $progressTotal += ($progressPercent * $weight) / 100;
        }

        return round($progressTotal, 2);
    }
}

