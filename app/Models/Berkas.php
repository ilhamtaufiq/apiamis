<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class Berkas extends Model implements HasMedia
{
    use InteractsWithMedia;

    protected $table = 'tbl_berkas';

    protected $fillable = [
        'pekerjaan_id',
        'jenis_dokumen',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    public function registerMediaCollections(): void
    {
        $this
            ->addMediaCollection('berkas/dokumen')
            ->useFallbackUrl('/fallback-berkas.pdf'); // Optional default file
    }
}
