<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class Penerima extends Model
{
    protected $table = 'tbl_penerima';

    protected $fillable = [
        'pekerjaan_id',
        'nama',
        'jumlah_jiwa',
        'nik',
        'alamat',
        'is_komunal'
    ];

    protected $casts = [
        'pekerjaan_id' => 'integer',
        'jumlah_jiwa' => 'integer',
        'is_komunal' => 'boolean',
    ];

    /**
     * Relasi Many-to-One dengan Pekerjaan
     */
    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    /**
     * Scope untuk filter komunal
     */
    public function scopeKomunal($query, $isKomunal = true)
    {
        return $query->where('is_komunal', $isKomunal);
    }

    /**
     * Scope untuk search nama
     */
    public function scopeSearchNama($query, $search)
    {
        return $query->where('nama', 'like', "%{$search}%");
    }
}
