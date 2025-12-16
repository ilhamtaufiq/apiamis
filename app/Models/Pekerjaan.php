<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;

class Pekerjaan extends Model
{
    /**
     * Scope untuk filter berdasarkan role user
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
        
        // Ambil kegiatan_id yang diizinkan untuk semua role user
        $allowedKegiatanIds = [];
        
        foreach ($user->roles as $role) {
            $kegiatanIds = KegiatanRole::where('role_id', $role->id)
                ->pluck('kegiatan_id')
                ->toArray();
            $allowedKegiatanIds = array_merge($allowedKegiatanIds, $kegiatanIds);
        }
        
        $allowedKegiatanIds = array_unique($allowedKegiatanIds);
        
        return $query->whereIn('kegiatan_id', $allowedKegiatanIds);
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