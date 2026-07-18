<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\BelongsToMany;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Schema;

use App\Services\SpamPekerjaanIntegrationService;
use App\Services\SpmSanitasiPekerjaanIntegrationService;
use App\Traits\Auditable;
use App\Traits\BroadcastsPekerjaanRealtime;
use App\Traits\NotifiesAdminsOnChanges;

class Pekerjaan extends Model
{
    use BroadcastsPekerjaanRealtime, NotifiesAdminsOnChanges, Auditable;

    /**
     * Unit SPAM / SPM Sanitasi yang tertaut, disimpan sebelum cascade delete pivot.
     *
     * @var array<int, array{unit_spam: list<int>, spm_sanitasi: list<int>}>
     */
    private static array $pendingLinkResync = [];

    protected static function booted(): void
    {
        // Saat pekerjaan dihapus, pivot tautan cascade otomatis (FK).
        // Capaian yang pernah di-sync dari tautan harus dihitung ulang (sama seperti detach).
        static::deleting(function (Pekerjaan $pekerjaan) {
            $unitSpamIds = [];
            $spmSanitasiIds = [];

            if (Schema::hasTable('tbl_unit_spam_pekerjaan')) {
                $unitSpamIds = DB::table('tbl_unit_spam_pekerjaan')
                    ->where('pekerjaan_id', $pekerjaan->id)
                    ->pluck('unit_spam_id')
                    ->map(fn ($id) => (int) $id)
                    ->unique()
                    ->values()
                    ->all();
            }

            if (Schema::hasTable('tbl_spm_sanitasi_pekerjaan')) {
                $spmSanitasiIds = DB::table('tbl_spm_sanitasi_pekerjaan')
                    ->where('pekerjaan_id', $pekerjaan->id)
                    ->pluck('spm_sanitasi_id')
                    ->map(fn ($id) => (int) $id)
                    ->unique()
                    ->values()
                    ->all();
            }

            self::$pendingLinkResync[$pekerjaan->id] = [
                'unit_spam' => $unitSpamIds,
                'spm_sanitasi' => $spmSanitasiIds,
            ];
        });

        static::deleted(function (Pekerjaan $pekerjaan) {
            $pending = self::$pendingLinkResync[$pekerjaan->id] ?? null;
            unset(self::$pendingLinkResync[$pekerjaan->id]);

            if (! $pending) {
                return;
            }

            try {
                if (! empty($pending['unit_spam'])) {
                    $spamService = app(SpamPekerjaanIntegrationService::class);
                    foreach ($pending['unit_spam'] as $unitId) {
                        $unit = UnitSpam::query()->find($unitId);
                        if ($unit) {
                            $spamService->syncUnitAccumulationFromLinks($unit);
                        }
                    }
                }

                if (! empty($pending['spm_sanitasi'])) {
                    $sanitasiService = app(SpmSanitasiPekerjaanIntegrationService::class);
                    foreach ($pending['spm_sanitasi'] as $spmId) {
                        $spm = SpmSanitasi::query()->find($spmId);
                        if ($spm) {
                            $sanitasiService->syncInfrastrukturFromLinkedPekerjaan($spm);
                        }
                    }
                }
            } catch (\Throwable $e) {
                Log::warning('Gagal sync tautan setelah hapus pekerjaan', [
                    'pekerjaan_id' => $pekerjaan->id,
                    'error' => $e->getMessage(),
                ]);
            }
        });
    }

    /**
     * Scope untuk filter berdasarkan role user.
     *
     * - admin / manager / super-admin: lihat semua
     * - operator: lihat semua (termasuk bila juga punya role pengawas)
     * - pengawas / konsultan_pengawas: HANYA user_pekerjaan (assign manual)
     *   → tidak lewat kegiatan_role (sering bocor: 1 role = seluruh kegiatan)
     * - role lain (tfl, …): user_pekerjaan ATAU kegiatan_role
     */
    public function scopeByUserRole($query)
    {
        $user = auth()->user();

        if (! $user) {
            return $query->whereRaw('1=0');
        }

        // Full access — operator checked before pengawas so dual-role is not restricted.
        if ($user->hasAnyRole(['admin', 'manager', 'super-admin', 'operator'])) {
            return $query;
        }

        $tableName = $this->getTable();

        // Pengawas lapangan & konsultan: ketat per assignment user.
        if ($user->hasAnyRole(['pengawas', 'konsultan_pengawas'])) {
            return $query->whereIn("$tableName.id", function ($sub) use ($user) {
                $sub->select('pekerjaan_id')
                    ->from('user_pekerjaan')
                    ->where('user_id', $user->id);
            });
        }

        return $query->where(function ($q) use ($user, $tableName) {
            // 1. Assign manual
            $q->whereIn("$tableName.id", function ($sub) use ($user) {
                $sub->select('pekerjaan_id')
                    ->from('user_pekerjaan')
                    ->where('user_id', $user->id);
            })
                // 2. Akses sektoral via kegiatan_role (bukan untuk pengawas lapangan)
                ->orWhereIn("$tableName.kegiatan_id", function ($sub) use ($user) {
                    $userRoleIds = $user->roles()->pluck('id')->toArray();
                    if ($userRoleIds === []) {
                        $sub->selectRaw('0')->whereRaw('1=0');

                        return;
                    }
                    $sub->select('kegiatan_id')
                        ->from('kegiatan_role')
                        ->whereIn('role_id', $userRoleIds);
                });
        });
    }

    /**
     * Apakah user boleh mengakses satu pekerjaan (sama logika scopeByUserRole).
     */
    public static function userCanAccess(int $pekerjaanId, $user = null): bool
    {
        $user = $user ?? auth()->user();
        if (! $user) {
            return false;
        }

        return static::query()
            ->whereKey($pekerjaanId)
            ->byUserRole()
            ->exists();
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

    public const STATUS_ACTIVE = 'active';

    public const STATUS_CANCELED = 'canceled';

    /**
     * Paket yang masih relevan ditindaklanjuti (bukan dibatalkan).
     * NULL status dianggap active (legacy).
     */
    public function scopeNotCanceled($query)
    {
        return $query->where(function ($q) {
            $q->whereNull('status')
                ->orWhere('status', '!=', self::STATUS_CANCELED);
        });
    }

    protected $fillable = [
        'kode_rekening',
        'nama_paket',
        'kecamatan_id',
        'desa_id',
        'kegiatan_id',
        'pagu',
        'is_konsultan',
        'status',
        'catatan',
        'pengawas_id',
        'pendamping_id',
    ];

    protected $casts = [
        'pagu' => 'float',
        'is_konsultan' => 'boolean',
        'kecamatan_id' => 'integer',
        'desa_id' => 'integer',
        'kegiatan_id' => 'integer',
        'pengawas_id' => 'integer',
        'pendamping_id' => 'integer',
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

    public function beritaAcara(): \Illuminate\Database\Eloquent\Relations\HasOne
    {
        return $this->hasOne(BeritaAcara::class, 'pekerjaan_id');
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

    public function progressEstimasi(): HasMany
    {
        return $this->hasMany(PekerjaanProgressEstimasi::class, 'pekerjaan_id');
    }

    public function progressEstimasiHistory(): HasMany
    {
        return $this->hasMany(PekerjaanProgressEstimasiHistory::class, 'pekerjaan_id');
    }



    /**
     * Relasi Many-to-Many dengan Tags
     */
    public function spmSanitasi(): BelongsToMany
    {
        return $this->belongsToMany(SpmSanitasi::class, 'tbl_spm_sanitasi_pekerjaan', 'pekerjaan_id', 'spm_sanitasi_id')
            ->withPivot('output_id')
            ->withTimestamps();
    }

    public function unitSpam(): BelongsToMany
    {
        return $this->belongsToMany(UnitSpam::class, 'tbl_unit_spam_pekerjaan', 'pekerjaan_id', 'unit_spam_id')
            ->withPivot(['output_id', 'capaian_metric'])
            ->withTimestamps();
    }

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
