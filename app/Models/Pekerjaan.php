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
            return $query->whereRaw('1=0'); // No access
        }
        
        if ($user->hasRole('admin')) {
            return $query; // Admin lihat semua
        }
        
        // 1. Get manually assigned work IDs
        $assignedPekerjaanIds = $user->assignedPekerjaan()->pluck('tbl_pekerjaan.id')->toArray();
        
        // 2. Get activities assigned via role
        $userRoleIds = $user->roles()->pluck('id')->toArray();
        $kegiatanIds = \App\Models\KegiatanRole::whereIn('role_id', $userRoleIds)->pluck('kegiatan_id')->toArray();
        
        return $query->where(function($q) use ($assignedPekerjaanIds, $kegiatanIds) {
            $q->whereIn('id', $assignedPekerjaanIds)
              ->orWhereIn('kegiatan_id', $kegiatanIds);
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
        'pagu'
    ];

    protected $casts = [
        'pagu' => 'float',
        'kecamatan_id' => 'integer',
        'desa_id' => 'integer',
        'kegiatan_id' => 'integer'
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
     * Relasi One-to-One dengan BeritaAcara
     */
    public function beritaAcara(): \Illuminate\Database\Eloquent\Relations\HasOne
    {
        return $this->hasOne(BeritaAcara::class, 'pekerjaan_id');
    }
}