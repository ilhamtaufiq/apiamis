<?php

namespace App\Services;

use App\Models\DocumentRegister;
use App\Models\Kontrak;

class KontrakBapContextService
{
    private const REGISTER_CODES = [
        'bastp' => 'BASTP',
        'jaminan_uang_muka' => 'JAMINAN_UM',
        'uang_muka' => 'UANG_MUKA',
    ];

    public function __construct(
        private readonly KontrakAddendumRegisterGapService $registerGapService,
    ) {}

    public function build(Kontrak $kontrak): array
    {
        $kontrak->loadMissing([
            'pekerjaans',
            'latestApprovedAddendum',
            'addendums',
            'registers.type',
        ]);

        $bastp = $this->findRegisterByCode($kontrak, self::REGISTER_CODES['bastp']);
        $jaminanUangMuka = $this->findRegisterByCode($kontrak, self::REGISTER_CODES['jaminan_uang_muka']);
        $uangMuka = $this->findRegisterByCode($kontrak, self::REGISTER_CODES['uang_muka']);
        $addendum = $kontrak->latestApprovedAddendum;
        $pekerjaan = $kontrak->pekerjaans->first();
        $registerGaps = $this->registerGapService->findGapsForKontrak($kontrak)['items'];
        $pendingAddendums = $kontrak->addendums
            ->where('status', '!=', 'disetujui')
            ->sortByDesc('addendum_ke')
            ->values();

        $nilaiKontrakEfektif = $kontrak->nilaiKontrakBerjalan();

        $missing = [];
        if (! $bastp) {
            $missing[] = 'bastp';
        }

        return [
            'can_generate' => empty($missing),
            'missing' => $missing,
            'nilai_kontrak_efektif' => $nilaiKontrakEfektif,
            'nilai_kontrak_awal' => $kontrak->nilai_kontrak,
            'bastp' => $this->serializeRegister($bastp),
            'addendum' => $addendum ? [
                'id' => $addendum->id,
                'addendum_ke' => $addendum->addendum_ke,
                'nomor' => $addendum->nomor_addendum,
                'tanggal' => $addendum->tanggal_addendum?->format('Y-m-d'),
                'nilai_kontrak_sesudah' => $addendum->nilai_kontrak_sesudah,
            ] : null,
            'addendum_register_gaps' => $registerGaps,
            'pending_addendums' => $pendingAddendums
                ->map(fn ($item) => [
                    'id' => $item->id,
                    'addendum_ke' => $item->addendum_ke,
                    'nomor' => $item->nomor_addendum,
                    'tanggal' => $item->tanggal_addendum?->format('Y-m-d'),
                    'status' => $item->status,
                    'nilai_kontrak_sesudah' => $item->nilai_kontrak_sesudah,
                ])
                ->all(),
            'jaminan_uang_muka' => $this->serializeRegister($jaminanUangMuka),
            'uang_muka' => $this->serializeRegister($uangMuka),
            'pekerjaan' => $pekerjaan ? [
                'id' => $pekerjaan->id,
                'nama_paket' => $pekerjaan->nama_paket,
            ] : null,
        ];
    }

    private function findRegisterByCode(Kontrak $kontrak, string $code): ?DocumentRegister
    {
        $normalized = strtoupper($code);

        return $kontrak->registers->first(function (DocumentRegister $register) use ($normalized) {
            return strtoupper((string) ($register->type?->code ?? '')) === $normalized;
        });
    }

    private function serializeRegister(?DocumentRegister $register): ?array
    {
        if (! $register) {
            return null;
        }

        return [
            'register_id' => $register->id,
            'nomor' => $register->nomor,
            'tanggal' => $register->tanggal?->format('Y-m-d'),
            'nilai' => $register->nilai !== null ? (float) $register->nilai : null,
            'type_code' => $register->type?->code,
            'type_name' => $register->type?->name,
        ];
    }
}