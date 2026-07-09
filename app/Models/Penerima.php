<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

use App\Traits\Auditable;
use App\Traits\BroadcastsPekerjaanRealtime;
use App\Traits\NotifiesAdminsOnChanges;

class Penerima extends Model
{
    use BroadcastsPekerjaanRealtime, NotifiesAdminsOnChanges, Auditable;
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
        'nik' => 'encrypted',
        'alamat' => 'encrypted',
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
