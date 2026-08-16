<?php

namespace App\Services;

use App\Models\DocumentRegister;
use App\Models\DocumentType;
use App\Models\Kontrak;

class KontrakAddendumRegisterGapService
{
    private const ADDENDUM_TYPE_CODES = ['add', 'addendum'];

    public function findGaps(): array
    {
        $typeIds = $this->resolveAddendumTypeIds();

        if ($typeIds->isEmpty()) {
            return [
                'total' => 0,
                'items' => [],
                'type_codes' => self::ADDENDUM_TYPE_CODES,
            ];
        }

        $registers = DocumentRegister::query()
            ->with([
                'type',
                'kontrak.pekerjaan.pengawas',
                'kontrak.pekerjaans',
                'kontrak.penyedia',
                'kontrak.addendums',
            ])
            ->whereIn('type_id', $typeIds)
            ->whereNull('addendum_id')
            ->orderByDesc('tanggal')
            ->get();

        $items = [];

        foreach ($registers as $register) {
            $kontrak = $register->kontrak;
            if (! $kontrak) {
                continue;
            }

            $registerNomor = $this->normalizeNomor($register->nomor);
            if ($registerNomor === '') {
                continue;
            }

            // Addendum sudah dibuat (meski nomor belum ditetapkan) — jangan tampilkan sebagai gap.
            if ($kontrak->addendums->isNotEmpty()) {
                continue;
            }

            $pekerjaan = $kontrak->pekerjaan;

            $items[] = [
                'register_id' => $register->id,
                'nomor_register' => $register->nomor,
                'tanggal_register' => $register->tanggal?->format('Y-m-d'),
                'type_code' => $register->type?->code,
                'type_name' => $register->type?->name,
                // Keterangan/nilai dari register — petunjuk untuk melengkapi addendum.
                'description' => $register->description,
                'nilai' => $register->nilai,
                'kontrak_id' => $kontrak->id,
                'addendum_id' => $register->addendum_id,
                'addendum_count' => $kontrak->addendums->count(),
                'pekerjaan' => $pekerjaan ? [
                    'id' => $pekerjaan->id,
                    'nama_paket' => $pekerjaan->nama_paket,
                    'kode_rekening' => $pekerjaan->kode_rekening,
                ] : null,
                'pekerjaans' => $kontrak->relationLoaded('pekerjaans')
                    ? $kontrak->pekerjaans->map(fn ($p) => [
                        'id' => $p->id,
                        'nama_paket' => $p->nama_paket,
                        'kode_rekening' => $p->kode_rekening,
                    ])->values()->all()
                    : [],
                'penyedia' => $kontrak->penyedia ? [
                    'id' => $kontrak->penyedia->id,
                    'nama' => $kontrak->penyedia->nama,
                ] : null,
                'pengawas' => $pekerjaan?->pengawas ? [
                    'id' => $pekerjaan->pengawas->id,
                    'nama' => $pekerjaan->pengawas->nama,
                ] : null,
            ];
        }

        return [
            'total' => count($items),
            'items' => $items,
            'type_codes' => self::ADDENDUM_TYPE_CODES,
        ];
    }

    public function findGapsForKontrak(Kontrak $kontrak): array
    {
        $result = $this->findGaps();

        $items = collect($result['items'])
            ->where('kontrak_id', $kontrak->id)
            ->values()
            ->all();

        return [
            'total' => count($items),
            'items' => $items,
            'type_codes' => $result['type_codes'],
        ];
    }

    private function resolveAddendumTypeIds()
    {
        return DocumentType::query()
            ->get()
            ->filter(fn (DocumentType $type) => $this->isAddendumTypeCode($type->code))
            ->pluck('id');
    }

    private function isAddendumTypeCode(?string $code): bool
    {
        $normalized = strtolower(trim((string) $code));

        return in_array($normalized, self::ADDENDUM_TYPE_CODES, true);
    }

    private function normalizeNomor(?string $nomor): string
    {
        $value = trim((string) $nomor);

        if ($value === '') {
            return '';
        }

        return strtoupper(preg_replace('/\s+/', ' ', $value));
    }
}