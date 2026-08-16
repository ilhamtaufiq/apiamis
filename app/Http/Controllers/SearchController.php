<?php

namespace App\Http\Controllers;

use App\Models\Desa;
use App\Models\Foto;
use App\Models\Kegiatan;
use App\Models\Kontrak;
use App\Models\Output;
use App\Models\Pekerjaan;
use App\Models\Penerima;
use App\Models\Penyedia;
use App\Models\Progress;
use App\Models\User;
use Illuminate\Http\Request;

class SearchController extends Controller
{
    /**
     * Global Search
     *
     * @OA\Get(
     *     path="/api/search",
     *     summary="Global Search",
     *     description="Search across multiple entities (Pekerjaan, Kontrak, Penyedia, Kegiatan, User, Desa) globally.",
     *     operationId="globalSearch",
     *     tags={"Search"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *
     *     @OA\Parameter(
     *         name="q",
     *         in="query",
     *         description="Search query",
     *         required=true,
     *
     *         @OA\Schema(type="string")
     *     ),
     *
     *     @OA\Response(
     *         response=200,
     *         description="Search Results"
     *     )
     * )
     */
    public function index(Request $request)
    {
        $query = $request->query('q');
        $tahun = $request->query('tahun', date('Y'));

        if (! $query) {
            return response()->json(['success' => true, 'data' => []]);
        }

        // Batasi panjang query agar full-text scan tak liar.
        $query = mb_substr(trim((string) $query), 0, 60);

        $results = array_merge(
            $this->searchPekerjaan($query, $tahun),
            $this->searchKontrak($query, $tahun),
            $this->searchPenyedia($query),
            $this->searchKegiatan($query),
            $this->searchDesa($query),
            $this->searchFoto($query, $tahun),
            $this->searchPenerima($query, $tahun),
            $this->searchOutput($query, $tahun),
            $this->searchProgress($query, $tahun)
        );

        return response()->json([
            'success' => true,
            'data' => collect($results)->sortBy('type')->values()->all(),
        ]);
    }

    /** Escape karakter operator boolean-mode agar query tak error/berperilaku liar. */
    private function escapeBool(string $query): string
    {
        return str_replace(['\\', '+', '-', '>', '<', '(', ')', '~', '*', '"', '@'], ' ', $query);
    }

    private function searchPekerjaan(string $query, ?string $tahun): array
    {
        return Pekerjaan::byUserRole()
            ->where(function ($q) use ($query) {
                $q->whereRaw('MATCH(nama_paket, kode_rekening) AGAINST(? IN BOOLEAN MODE)', [$this->escapeBool($query)])
                    ->orWhereHas('kontrak.penyedia', function ($penyediaQuery) use ($query) {
                        $penyediaQuery->where('nama', 'LIKE', "%{$query}%");
                    });
            })
            ->whereHas('kegiatan', function ($q) use ($tahun) {
                if ($tahun) {
                    $q->where('tahun_anggaran', $tahun);
                }
            })
            ->with(['desa', 'kecamatan', 'kegiatan', 'kontrak.penyedia'])
            ->limit(10)
            ->get()
            ->map(function ($item) use ($query, $tahun) {
                $penyedia = optional($item->kontrak->first())->penyedia;
                $mapSearch = $penyedia && stripos($penyedia->nama, $query) !== false
                    ? $penyedia->nama
                    : $item->nama_paket;

                return [
                    'id' => $item->id,
                    'type' => 'Pekerjaan',
                    'title' => $item->nama_paket,
                    'subtitle' => trim(($item->kode_rekening ?? '').' - '.($item->desa->n_desa ?? '').($penyedia ? ' - '.$penyedia->nama : ''), ' -'),
                    'tahun' => $item->kegiatan->tahun_anggaran ?? null,
                    'url' => "/pekerjaan/{$item->id}",
                    'map_url' => '/map?search='.rawurlencode($mapSearch).($tahun ? '&tahun='.rawurlencode($tahun) : ''),
                ];
            })->toArray();
    }

    private function searchKontrak(string $query, ?string $tahun): array
    {
        return Kontrak::where(function ($q) use ($query) {
            $q->whereRaw('MATCH(spk, spmk, kode_paket) AGAINST(? IN BOOLEAN MODE)', [$this->escapeBool($query)])
                ->orWhereHas('pekerjaan', function ($pq) use ($query) {
                    $pq->byUserRole()->where('nama_paket', 'LIKE', "%{$query}%");
                });
        })
            ->whereHas('pekerjaan.kegiatan', function ($q) use ($tahun) {
                if ($tahun) {
                    $q->where('tahun_anggaran', $tahun);
                }
            })
            ->with(['pekerjaan.kegiatan', 'penyedia'])
            ->limit(15)
            ->get()
            ->map(fn ($item) => [
                'id' => $item->id,
                'type' => 'Kontrak',
                'title' => $item->spk ?? $item->kode_paket,
                'subtitle' => ($item->pekerjaan->nama_paket ?? 'N/A'),
                'penyedia' => $item->penyedia->nama ?? 'N/A',
                'nilai' => $item->nilai_kontrak,
                'tahun' => $item->pekerjaan->kegiatan->tahun_anggaran ?? null,
                'url' => "/kontrak/{$item->id}",
            ])->toArray();
    }

    private function searchPenyedia(string $query): array
    {
        return Penyedia::whereRaw('MATCH(nama, direktur) AGAINST(? IN BOOLEAN MODE)', [$this->escapeBool($query)])
            ->limit(5)
            ->get()
            ->map(fn ($item) => [
                'id' => $item->id,
                'type' => 'Penyedia',
                'title' => $item->nama,
                'subtitle' => 'Direktur: '.$item->direktur,
                'url' => "/penyedia/{$item->id}",
                'map_url' => '/map?search='.rawurlencode($item->nama),
            ])->toArray();
    }

    private function searchKegiatan(string $query): array
    {
        return Kegiatan::whereRaw('MATCH(nama_kegiatan, nama_sub_kegiatan, nama_program) AGAINST(? IN BOOLEAN MODE)', [$this->escapeBool($query)])
            ->limit(5)
            ->get()
            ->map(fn ($item) => [
                'id' => $item->id,
                'type' => 'Kegiatan',
                'title' => $item->nama_kegiatan,
                'subtitle' => $item->nama_sub_kegiatan,
                'url' => "/kegiatan/{$item->id}",
            ])->toArray();
    }

    private function searchDesa(string $query): array
    {
        return Desa::whereRaw('MATCH(n_desa) AGAINST(? IN BOOLEAN MODE)', [$this->escapeBool($query)])
            ->with('kecamatan')
            ->limit(5)
            ->get()
            ->map(fn ($item) => [
                'id' => $item->id,
                'type' => 'Desa',
                'title' => 'Desa '.$item->n_desa,
                'subtitle' => 'Kec. '.($item->kecamatan->n_kec ?? ''),
                'url' => "/desa/{$item->id}",
            ])->toArray();
    }

    private function searchFoto(string $query, ?string $tahun): array
    {
        return Foto::where(function ($q) use ($query) {
            $q->where('keterangan', 'like', "%{$query}%")
                ->orWhereHas('pekerjaan', function ($pq) use ($query) {
                    $pq->byUserRole()->where('nama_paket', 'LIKE', "%{$query}%");
                });
        })
            ->whereHas('pekerjaan.kegiatan', function ($q) use ($tahun) {
                if ($tahun) {
                    $q->where('tahun_anggaran', $tahun);
                }
            })
            ->with(['pekerjaan.kegiatan'])
            ->limit(10)
            ->get()
            ->map(fn (Foto $item) => [
                'id' => $item->id,
                'type' => 'Dokumentasi',
                'title' => 'Dokumentasi: '.($item->keterangan ?? 'Tanpa Keterangan'),
                'subtitle' => 'Pekerjaan: '.($item->pekerjaan->nama_paket ?? ''),
                'tahun' => $item->pekerjaan->kegiatan->tahun_anggaran ?? null,
                'image_url' => $item->getFirstMediaUrl('foto/pekerjaan'),
                'url' => "/pekerjaan/{$item->pekerjaan_id}",
            ])->toArray();
    }

    private function searchPenerima(string $query, ?string $tahun): array
    {
        return Penerima::where(function ($q) use ($query) {
            $q->whereRaw('MATCH(nama, nik, alamat) AGAINST(? IN BOOLEAN MODE)', [$this->escapeBool($query)])
                ->orWhereHas('pekerjaan', function ($pq) use ($query) {
                    $pq->byUserRole()->where('nama_paket', 'LIKE', "%{$query}%");
                });
        })
            ->whereHas('pekerjaan.kegiatan', function ($q) use ($tahun) {
                if ($tahun) {
                    $q->where('tahun_anggaran', $tahun);
                }
            })
            ->with(['pekerjaan.kegiatan'])
            ->limit(10)
            ->get()
            ->map(fn ($item) => [
                'id' => $item->id,
                'type' => 'Penerima Manfaat',
                'title' => 'Penerima: '.$item->nama,
                'subtitle' => 'Alamat: '.($item->alamat ?? '').' | Pekerjaan: '.($item->pekerjaan->nama_paket ?? ''),
                'tahun' => $item->pekerjaan->kegiatan->tahun_anggaran ?? null,
                'url' => "/pekerjaan/{$item->pekerjaan_id}",
            ])->toArray();
    }

    private function searchOutput(string $query, ?string $tahun): array
    {
        return Output::where(function ($q) use ($query) {
            $q->whereRaw('MATCH(komponen, satuan) AGAINST(? IN BOOLEAN MODE)', [$this->escapeBool($query)])
                ->orWhereHas('pekerjaan', function ($pq) use ($query) {
                    $pq->byUserRole()->where('nama_paket', 'LIKE', "%{$query}%");
                });
        })
            ->whereHas('pekerjaan.kegiatan', function ($q) use ($tahun) {
                if ($tahun) {
                    $q->where('tahun_anggaran', $tahun);
                }
            })
            ->with(['pekerjaan.kegiatan'])
            ->limit(10)
            ->get()
            ->map(fn ($item) => [
                'id' => $item->id,
                'type' => 'Output',
                'title' => 'Output: '.$item->komponen,
                'subtitle' => 'Volume: '.$item->volume.' '.$item->satuan.' | Pekerjaan: '.($item->pekerjaan->nama_paket ?? ''),
                'tahun' => $item->pekerjaan->kegiatan->tahun_anggaran ?? null,
                'url' => "/pekerjaan/{$item->pekerjaan_id}",
            ])->toArray();
    }

    private function searchProgress(string $query, ?string $tahun): array
    {
        return Progress::where(function ($q) use ($query) {
            $q->where('content', 'like', "%{$query}%")
                ->orWhereHas('pekerjaan', function ($pq) use ($query) {
                    $pq->byUserRole()->where('nama_paket', 'LIKE', "%{$query}%");
                });
        })
            // content adalah JSON besar; batasi scan via pekerjaan dulu bila mungkin.
            ->whereHas('pekerjaan')
            ->whereHas('pekerjaan.kegiatan', function ($q) use ($tahun) {
                if ($tahun) {
                    $q->where('tahun_anggaran', $tahun);
                }
            })
            ->with(['pekerjaan.kegiatan'])
            ->limit(10)
            ->get()
            ->map(fn ($item) => [
                'id' => $item->id,
                'type' => 'Progress',
                'title' => 'Progress Log Entry',
                'subtitle' => 'Pekerjaan: '.($item->pekerjaan->nama_paket ?? ''),
                'tahun' => $item->pekerjaan->kegiatan->tahun_anggaran ?? null,
                'url' => "/pekerjaan/{$item->pekerjaan_id}",
            ])->toArray();
    }
}
