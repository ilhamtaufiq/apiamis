<?php

namespace App\Services;

use App\Models\SpamKelembagaanShareLink;
use App\Models\SpamKelembagaanSubmission;
use App\Models\UnitSpam;
use Illuminate\Support\Arr;
use Illuminate\Support\Facades\DB;
use Illuminate\Validation\ValidationException;

class SpamKelembagaanShareService
{
    /** Field unit yang boleh diusulkan lewat form publik. */
    public const UNIT_FIELDS = [
        'name',
        'tahun_pembangunan',
        'sumber_dana',
        'program',
        'sistem_layanan',
        'sumber_mata_air_kap',
        'sumber_air_tanah_kap',
        'lain_lain_kap',
        'tarif_dasar_hukum',
        'iuran_nominal',
        'pendapatan_bulan',
        'biaya_operasional',
    ];

    /** Field pengelola (POKMAS). */
    public const PENGELOLA_FIELDS = [
        'pokmas',
        'perdes',
        'kepala',
        'bendahara',
        'sekretaris',
    ];

    public function sanitizePayload(array $input): array
    {
        $payload = [];
        foreach (array_merge(self::UNIT_FIELDS, self::PENGELOLA_FIELDS) as $field) {
            if (! array_key_exists($field, $input)) {
                continue;
            }
            $value = $input[$field];
            if ($value === null) {
                $payload[$field] = null;
                continue;
            }
            $payload[$field] = is_string($value) ? trim($value) : $value;
        }

        return $payload;
    }

    public function snapshotUnit(UnitSpam $unit): array
    {
        $unit->loadMissing('pengelola');
        $data = Arr::only($unit->toArray(), self::UNIT_FIELDS);
        foreach (self::PENGELOLA_FIELDS as $field) {
            $data[$field] = $unit->pengelola?->{$field};
        }

        return $data;
    }

    public function publicFormData(SpamKelembagaanShareLink $link): array
    {
        $unit = $link->unitSpam()->with(['desa.kecamatan', 'pengelola'])->firstOrFail();

        return [
            'link' => [
                'token' => $link->token,
                'label' => $link->label,
                'expires_at' => $link->expires_at?->toIso8601String(),
                'is_usable' => $link->isUsable(),
            ],
            'unit' => [
                'id' => $unit->id,
                'name' => $unit->name,
                'desa' => $unit->desa?->n_desa ?? $unit->desa?->nama_desa,
                'kecamatan' => $unit->desa?->kecamatan?->n_kec
                    ?? $unit->desa?->kecamatan?->nama_kecamatan,
                'current' => $this->snapshotUnit($unit),
            ],
            'fields' => [
                'unit' => self::UNIT_FIELDS,
                'pengelola' => self::PENGELOLA_FIELDS,
            ],
        ];
    }

    public function createSubmission(
        SpamKelembagaanShareLink $link,
        array $payload,
        array $meta
    ): SpamKelembagaanSubmission {
        if (! $link->isUsable()) {
            throw ValidationException::withMessages([
                'token' => ['Link form tidak aktif, kedaluwarsa, atau kuota sudah penuh.'],
            ]);
        }

        $clean = $this->sanitizePayload($payload);
        if ($clean === []) {
            throw ValidationException::withMessages([
                'payload' => ['Tidak ada data yang diisi untuk diusulkan.'],
            ]);
        }

        $unit = $link->unitSpam()->with('pengelola')->firstOrFail();

        return DB::transaction(function () use ($link, $unit, $clean, $meta) {
            $submission = SpamKelembagaanSubmission::create([
                'share_link_id' => $link->id,
                'unit_spam_id' => $unit->id,
                'payload' => $clean,
                'snapshot_before' => $this->snapshotUnit($unit),
                'submitter_name' => $meta['submitter_name'] ?? null,
                'submitter_phone' => $meta['submitter_phone'] ?? null,
                'submitter_instansi' => $meta['submitter_instansi'] ?? null,
                'submitter_note' => $meta['submitter_note'] ?? null,
                'status' => SpamKelembagaanSubmission::STATUS_PENDING,
                'submitter_ip' => $meta['submitter_ip'] ?? null,
                'user_agent' => $meta['user_agent'] ?? null,
            ]);

            $link->increment('submission_count');

            return $submission->load(['unitSpam.desa.kecamatan', 'shareLink']);
        });
    }

    public function approve(SpamKelembagaanSubmission $submission, int $reviewerId, ?string $note = null): SpamKelembagaanSubmission
    {
        if (! $submission->isPending()) {
            throw ValidationException::withMessages([
                'status' => ['Usulan ini sudah diproses sebelumnya.'],
            ]);
        }

        return DB::transaction(function () use ($submission, $reviewerId, $note) {
            $unit = UnitSpam::with('pengelola')->findOrFail($submission->unit_spam_id);
            $payload = $submission->payload ?? [];

            $unitData = Arr::only($payload, self::UNIT_FIELDS);
            if ($unitData !== []) {
                $unit->update($unitData);
            }

            $pengelolaData = Arr::only($payload, self::PENGELOLA_FIELDS);
            if ($pengelolaData !== []) {
                if ($unit->pengelola) {
                    $unit->pengelola->update($pengelolaData);
                } else {
                    $unit->pengelola()->create($pengelolaData);
                }
            }

            $submission->update([
                'status' => SpamKelembagaanSubmission::STATUS_APPROVED,
                'reviewed_by' => $reviewerId,
                'reviewed_at' => now(),
                'review_note' => $note,
            ]);

            return $submission->fresh(['unitSpam.desa.kecamatan', 'unitSpam.pengelola', 'reviewer', 'shareLink']);
        });
    }

    public function reject(SpamKelembagaanSubmission $submission, int $reviewerId, ?string $note = null): SpamKelembagaanSubmission
    {
        if (! $submission->isPending()) {
            throw ValidationException::withMessages([
                'status' => ['Usulan ini sudah diproses sebelumnya.'],
            ]);
        }

        $submission->update([
            'status' => SpamKelembagaanSubmission::STATUS_REJECTED,
            'reviewed_by' => $reviewerId,
            'reviewed_at' => now(),
            'review_note' => $note,
        ]);

        return $submission->fresh(['unitSpam.desa.kecamatan', 'reviewer', 'shareLink']);
    }

    public function serializeSubmission(SpamKelembagaanSubmission $submission): array
    {
        $submission->loadMissing(['unitSpam.desa.kecamatan', 'shareLink', 'reviewer']);

        return [
            'id' => $submission->id,
            'share_link_id' => $submission->share_link_id,
            'unit_spam_id' => $submission->unit_spam_id,
            'payload' => $submission->payload,
            'snapshot_before' => $submission->snapshot_before,
            'submitter_name' => $submission->submitter_name,
            'submitter_phone' => $submission->submitter_phone,
            'submitter_instansi' => $submission->submitter_instansi,
            'submitter_note' => $submission->submitter_note,
            'status' => $submission->status,
            'review_note' => $submission->review_note,
            'reviewed_at' => $submission->reviewed_at?->toIso8601String(),
            'reviewer' => $submission->reviewer ? [
                'id' => $submission->reviewer->id,
                'name' => $submission->reviewer->name,
            ] : null,
            'created_at' => $submission->created_at?->toIso8601String(),
            'unit' => $submission->unitSpam ? [
                'id' => $submission->unitSpam->id,
                'name' => $submission->unitSpam->name,
                'desa' => $submission->unitSpam->desa?->n_desa ?? $submission->unitSpam->desa?->nama_desa,
                'kecamatan' => $submission->unitSpam->desa?->kecamatan?->n_kec
                    ?? $submission->unitSpam->desa?->kecamatan?->nama_kecamatan,
            ] : null,
            'share_link' => $submission->shareLink ? [
                'id' => $submission->shareLink->id,
                'token' => $submission->shareLink->token,
                'label' => $submission->shareLink->label,
            ] : null,
        ];
    }

    public function serializeLink(SpamKelembagaanShareLink $link): array
    {
        $link->loadMissing(['unitSpam.desa.kecamatan', 'creator']);

        return [
            'id' => $link->id,
            'token' => $link->token,
            'label' => $link->label,
            'is_active' => $link->is_active,
            'is_usable' => $link->isUsable(),
            'expires_at' => $link->expires_at?->toIso8601String(),
            'max_submissions' => $link->max_submissions,
            'submission_count' => $link->submission_count,
            'admin_note' => $link->admin_note,
            'path' => $link->publicUrlPath(),
            'created_at' => $link->created_at?->toIso8601String(),
            'unit_spam_id' => $link->unit_spam_id,
            'unit' => $link->unitSpam ? [
                'id' => $link->unitSpam->id,
                'name' => $link->unitSpam->name,
                'desa' => $link->unitSpam->desa?->n_desa ?? $link->unitSpam->desa?->nama_desa,
                'kecamatan' => $link->unitSpam->desa?->kecamatan?->n_kec
                    ?? $link->unitSpam->desa?->kecamatan?->nama_kecamatan,
            ] : null,
            'creator' => $link->creator ? [
                'id' => $link->creator->id,
                'name' => $link->creator->name,
            ] : null,
        ];
    }
}
