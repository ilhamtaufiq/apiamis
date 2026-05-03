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
        
        return $query->where(function($q) use ($user) {
            $tableName = $this->getTable();

            // 1. Manually assigned via user_pekerjaan table
            $q->whereIn("$tableName.id", function($sub) use ($user) {
                $sub->select('pekerjaan_id')
                    ->from('user_pekerjaan')
                    ->where('user_id', $user->id);
            })
            // 2. Assigned via kegiatan role (department/sector access)
            ->orWhereIn("$tableName.kegiatan_id", function($sub) use ($user) {
                $userRoleIds = $user->roles()->pluck('id')->toArray();
                $sub->select('kegiatan_id')
                    ->from('kegiatan_role')
                    ->whereIn('role_id', $userRoleIds);
            });

            // 3. Automatically assigned if user's NIP matches the Pengawas/Pendamping master data
            if ($user->nip) {
                $q->orWhere(function($sub) use ($user) {
                    $sub->whereHas('pengawas', function($p) use ($user) {
                        $p->where('nip', $user->nip);
                    })->orWhereHas('pendamping', function($p) use ($user) {
                        $p->where('nip', $user->nip);
                    });
                });
            }
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
     * Relasi One-to-Many dengan Kontrak
     */
    public function kontrak(): HasMany
    {
        return $this->hasMany(Kontrak::class, 'id_pekerjaan');
    }

    /**
     * Relasi One-to-One dengan Progress
     */
    public function progress(): \Illuminate\Database\Eloquent\Relations\HasOne
    {
        return $this->hasOne(Progress::class, 'pekerjaan_id');
    }

    /**
     * Relasi One-to-One dengan BeritaAcara
     */
    public function beritaAcara(): \Illuminate\Database\Eloquent\Relations\HasOne
    {
        return $this->hasOne(BeritaAcara::class, 'pekerjaan_id');
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

    public function draft(): \Illuminate\Database\Eloquent\Relations\HasOne
    {
        return $this->hasOne(DraftPekerjaan::class, 'pekerjaan_id');
    }
}