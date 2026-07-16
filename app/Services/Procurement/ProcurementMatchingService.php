<?php

namespace App\Services\Procurement;

use App\Models\DraftPekerjaan;
use App\Models\Kontrak;
use App\Models\Pekerjaan;
use App\Models\ProcurementStagingPaket;
use Illuminate\Support\Collection;

class ProcurementMatchingService
{
    /** @var array<string, Collection<int, Pekerjaan>> */
    private array $pekerjaanCache = [];

    public function matchStaging(ProcurementStagingPaket $staging): ProcurementStagingPaket
    {
        $kontrak = Kontrak::query()
            ->where('kode_paket', $staging->kode_paket)
            ->first();

        if ($kontrak) {
            $pekerjaan = $kontrak->pekerjaans()->first() ?? $kontrak->pekerjaan;

            return $this->applyMatch($staging, 'exact_kode_paket', $pekerjaan?->id, $kontrak->id);
        }

        $pekerjaan = $this->findPekerjaanByName($staging->nama_paket);
        if ($pekerjaan) {
            $kontrakFromPekerjaan = $pekerjaan->kontrak()->first();

            return $this->applyMatch(
                $staging,
                'fuzzy_nama_paket',
                $pekerjaan->id,
                $kontrakFromPekerjaan?->id,
            );
        }

        $staging->match_status = 'unmatched';
        $staging->matched_pekerjaan_id = null;
        $staging->matched_kontrak_id = null;
        $staging->save();

        return $staging;
    }

    public function applyToKontrak(ProcurementStagingPaket $staging, bool $overwrite = false): ?Kontrak
    {
        $kontrak = null;

        if ($staging->matched_kontrak_id) {
            $kontrak = Kontrak::query()->find($staging->matched_kontrak_id);
        }

        if (! $kontrak && $staging->matched_pekerjaan_id) {
            $pekerjaan = Pekerjaan::query()->find($staging->matched_pekerjaan_id);
            $kontrak = $pekerjaan?->kontrak()->first();
        }

        if (! $kontrak) {
            return null;
        }

        $updates = [];
        if ($overwrite || empty($kontrak->kode_paket)) {
            $updates['kode_paket'] = $staging->kode_paket;
        }

        if ($updates !== []) {
            $kontrak->update($updates);
        }

        return $kontrak->fresh();
    }

    /**
     * Create draft pekerjaan (+ kontrak shell) from unmatched staging paket.
     *
     * @param  array{kegiatan_id?: int|null, pagu?: float|null, is_konsultan?: bool}  $options
     */
    public function promoteToDraft(ProcurementStagingPaket $staging, array $options = []): array
    {
        if ($staging->matched_pekerjaan_id) {
            $existing = Pekerjaan::query()->find($staging->matched_pekerjaan_id);

            return [
                'status' => 'already_matched',
                'pekerjaan' => $existing,
                'kontrak' => $existing?->kontrak()->first(),
                'staging' => $staging,
            ];
        }

        $isKonsultan = (bool) ($options['is_konsultan'] ?? true);
        $pagu = isset($options['pagu']) ? (float) $options['pagu'] : 0.0;

        $pekerjaan = Pekerjaan::query()->create([
            'nama_paket' => mb_substr($staging->nama_paket ?: ('Paket SPSE '.$staging->kode_paket), 0, 225),
            'kode_rekening' => null,
            'kegiatan_id' => $options['kegiatan_id'] ?? null,
            'pagu' => $pagu,
            'is_konsultan' => $isKonsultan,
            'kecamatan_id' => null,
            'desa_id' => null,
        ]);

        $kontrak = Kontrak::query()->create([
            'id_pekerjaan' => $pekerjaan->id,
            'id_kegiatan' => $options['kegiatan_id'] ?? null,
            'kode_paket' => $staging->kode_paket,
            'kode_rup' => data_get($staging->raw_row, 'kode_rup')
                ?? data_get($staging->raw_row, 'kodeRup')
                ?? null,
            'nilai_kontrak' => $pagu > 0 ? $pagu : null,
        ]);

        DraftPekerjaan::query()->updateOrCreate(
            ['pekerjaan_id' => $pekerjaan->id],
            [
                'kode_paket' => $staging->kode_paket,
                'kode_rup' => data_get($staging->raw_row, 'kode_rup')
                    ?? data_get($staging->raw_row, 'kodeRup')
                    ?? null,
            ],
        );

        $this->applyMatch($staging, 'promoted_draft', $pekerjaan->id, $kontrak->id);

        return [
            'status' => 'created',
            'pekerjaan' => $pekerjaan->fresh(),
            'kontrak' => $kontrak->fresh(),
            'staging' => $staging->fresh(['pekerjaan', 'kontrak']),
        ];
    }

    private function applyMatch(
        ProcurementStagingPaket $staging,
        string $status,
        ?int $pekerjaanId,
        ?int $kontrakId,
    ): ProcurementStagingPaket {
        $staging->match_status = $status;
        $staging->matched_pekerjaan_id = $pekerjaanId;
        $staging->matched_kontrak_id = $kontrakId;
        $staging->save();

        return $staging;
    }

    private function findPekerjaanByName(string $namaPaket): ?Pekerjaan
    {
        $target = $this->normalizeLookupText($namaPaket);
        if ($target === '') {
            return null;
        }

        $candidates = $this->getPekerjaanCandidates();

        $exact = $candidates->first(
            fn (Pekerjaan $p) => $this->normalizeLookupText($p->nama_paket) === $target,
        );
        if ($exact) {
            return $exact;
        }

        return $candidates->first(function (Pekerjaan $p) use ($target) {
            $normalized = $this->normalizeLookupText($p->nama_paket);

            return str_contains($normalized, $target) || str_contains($target, $normalized);
        });
    }

    private function getPekerjaanCandidates(): Collection
    {
        if (! isset($this->pekerjaanCache['all'])) {
            $this->pekerjaanCache['all'] = Pekerjaan::query()->get();
        }

        return $this->pekerjaanCache['all'];
    }

    private function normalizeLookupText(?string $value): string
    {
        $value = preg_replace('/[^\pL\pN]+/u', '', trim((string) $value));

        return mb_strtolower($value);
    }
}