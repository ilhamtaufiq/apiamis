<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\BelongsToMany;
use Illuminate\Database\Eloquent\Relations\HasMany;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class Pekerjaan extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
    /**
     * Scope untuk filter berdasarkan role user
     * - Admin: lihat semua
     * - Pengawas/User lain: hanya lihat pekerjaan yang di-assign
     */
    public function scopeByUserRole($query)
    {
        $user = auth()->user();

        if (!$user) {
            return $query->whereRaw('1=0');
        }

        if ($user->hasRole('admin')) {
            return $query;
        }

        return $query->where(function ($q) use ($user) {
            $tableName = $this->getTable();

            // 1. Manually assigned via user_pekerjaan table
            $q->whereIn("$tableName.id", function ($sub) use ($user) {
                $sub->select('pekerjaan_id')
                    ->from('user_pekerjaan')
                    ->where('user_id', $user->id);
            })
                // 2. Assigned via kegiatan role (department/sector access)
                ->orWhereIn("$tableName.kegiatan_id", function ($sub) use ($user) {
                    $userRoleIds = $user->roles()->pluck('id')->toArray();
                    $sub->select('kegiatan_id')
                        ->from('kegiatan_role')
                        ->whereIn('role_id', $userRoleIds);
                });

            // 3. Automatically assigned if user's NIP matches the Pengawas/Pendamping master data
            // (Dihapus sesuai permintaan, sekarang menggunakan assignment manual)
        });
    }

    /**
     * Users yang di-assign ke pekerjaan ini (pengawas lapangan)
     */
    public function assignedUsers(): BelongsToMany
    {
        return $this->belongsToMany(User::class, 'user_pekerjaan', 'pekerjaan_id', 'user_id')
            ->withTimestamps();
    }
    protected $table = 'tbl_pekerjaan';

    protected $fillable = [
        'kode_rekening',
        'nama_paket',
        'kecamatan_id',
        'desa_id',
        'kegiatan_id',
        'pagu',
        'pengawas_id',
        'pendamping_id'
    ];

    protected $casts = [
        'pagu' => 'float',
        'kecamatan_id' => 'integer',
        'desa_id' => 'integer',
        'kegiatan_id' => 'integer',
        'pengawas_id' => 'integer',
        'pendamping_id' => 'integer'
    ];

    /**
     * Relasi Many-to-One dengan Kecamatan
     */
    public function kecamatan(): BelongsTo
    {
        return $this->belongsTo(Kecamatan::class, 'kecamatan_id');
    }

    /**
     * Relasi Many-to-One dengan Desa
     */
    public function desa(): BelongsTo
    {
        return $this->belongsTo(Desa::class, 'desa_id');
    }

    /**
     * Relasi Many-to-One dengan Kegiatan
     */
    public function kegiatan(): BelongsTo
    {
        return $this->belongsTo(Kegiatan::class, 'kegiatan_id');
    }

    /**
     * Relasi One-to-Many dengan Output
     */
    public function output(): HasMany
    {
        return $this->hasMany(Output::class, 'pekerjaan_id');
    }

    /**
     * Relasi One-to-Many dengan Penerima
     */
    public function penerima(): HasMany
    {
        return $this->hasMany(Penerima::class, 'pekerjaan_id');
    }
    public function berkas(): HasMany
    {
        return $this->hasMany(Berkas::class, 'pekerjaan_id');
    }

    /**
     * Relasi One-to-Many dengan Foto (langsung)
     */
    public function foto(): HasMany
    {
        return $this->hasMany(Foto::class, 'pekerjaan_id');
    }

    /**
     * Relasi Many-to-Many dengan Kontrak
     */
    public function kontrak(): \Illuminate\Database\Eloquent\Relations\BelongsToMany
    {
        return $this->belongsToMany(Kontrak::class, 'kontrak_pekerjaan', 'pekerjaan_id', 'kontrak_id')
            ->withTimestamps();
    }

    /**
     * Alias backward-compatible untuk pemanggilan lama.
     */
    public function kontraks(): \Illuminate\Database\Eloquent\Relations\BelongsToMany
    {
        return $this->kontrak();
    }

    /**
     * Relasi One-to-One dengan Progress
     */
    public function progress(): \Illuminate\Database\Eloquent\Relations\HasOne
    {
        return $this->hasOne(Progress::class, 'pekerjaan_id');
    }



    /**
     * Relasi Many-to-Many dengan Tags
     */
    public function tags(): BelongsToMany
    {
        return $this->belongsToMany(Tag::class, 'pekerjaan_tag', 'pekerjaan_id', 'tag_id')
            ->withTimestamps();
    }

    /**
     * Relasi Many-to-Many dengan ChecklistItems
     */
    public function checklistItems(): BelongsToMany
    {
        return $this->belongsToMany(ChecklistItem::class, 'pekerjaan_checklist', 'pekerjaan_id', 'checklist_item_id')
            ->withPivot(['is_checked', 'checked_at', 'checked_by', 'notes'])
            ->withTimestamps();
    }

    /**
     * Relasi ke Pengawas (Utama)
     */
    public function pengawas(): BelongsTo
    {
        return $this->belongsTo(Pengawas::class, 'pengawas_id');
    }

    /**
     * Relasi ke Pendamping
     */
    public function pendamping(): BelongsTo
    {
        return $this->belongsTo(Pengawas::class, 'pendamping_id');
    }

    public function isChecklistComplete(): bool
    {
        $total = $this->checklistItems()->count();
        if ($total === 0)
            return false; // Strict: must have checklist and must be complete

        $checked = $this->checklistItems()->wherePivot('is_checked', true)->count();
        return $checked === $total;
    }

    public function draft(): \Illuminate\Database\Eloquent\Relations\HasOne
    {
        return $this->hasOne(DraftPekerjaan::class, 'pekerjaan_id');
    }

    public function tiket(): HasMany
    {
        return $this->hasMany(Tiket::class, 'pekerjaan_id');
    }

    /**
     * @return array{foto_status: string|null, foto_count: int|null, foto_required_count: int|null}
     */
    public function resolveFotoMetrics(): array
    {
        $fotoActualCount = $this->relationLoaded('foto')
            ? $this->foto->count()
            : (isset($this->foto_count) ? (int) $this->foto_count : null);

        if (! $this->relationLoaded('output') || ! $this->relationLoaded('foto')) {
            $count = (int) ($fotoActualCount ?? 0);

            return [
                'foto_status' => $count > 0 ? 'belum_selesai' : 'belum_ada_foto',
                'foto_count' => $fotoActualCount,
                'foto_required_count' => null,
            ];
        }

        $fotoActualCount = $this->foto->count();
        $fotoRequiredCount = 0;
        $isComplete = true;
        $fotoByOutput = $this->foto->groupBy('komponen_id');

        if ($this->output->isEmpty()) {
            return [
                'foto_status' => $fotoActualCount > 0 ? 'belum_selesai' : 'belum_ada_foto',
                'foto_count' => $fotoActualCount,
                'foto_required_count' => 0,
            ];
        }

        foreach ($this->output as $output) {
            $outputPhotos = $fotoByOutput->get($output->id, collect());
            $outputPhotoCount = $outputPhotos->count();

            $requiredUnits = $output->penerima_is_optional
                ? 1
                : max(1, (int) ceil((float) ($output->volume ?? 0)));
            $requiredPhotos = $requiredUnits * 5;
            $fotoRequiredCount += $requiredPhotos;

            $distinctRecipients = $outputPhotos
                ->pluck('penerima_id')
                ->filter()
                ->unique()
                ->count();

            if ($outputPhotoCount < $requiredPhotos) {
                $isComplete = false;
            }

            if (! $output->penerima_is_optional && $distinctRecipients < $requiredUnits) {
                $isComplete = false;
            }
        }

        if ($fotoActualCount <= 0) {
            $fotoStatus = 'belum_ada_foto';
        } elseif (! $isComplete) {
            $fotoStatus = 'belum_selesai';
        } else {
            $fotoStatus = 'selesai';
        }

        return [
            'foto_status' => $fotoStatus,
            'foto_count' => $fotoActualCount,
            'foto_required_count' => $fotoRequiredCount,
        ];
    }
}
