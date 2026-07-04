<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class Penyedia extends Model implements HasMedia
{
    use NotifiesAdminsOnChanges, Auditable, InteractsWithMedia;
    protected $table = 'tbl_penyedia';
    
    protected $fillable = [
        'nama',
        'direktur',
        'no_akta',
        'notaris',
        'tanggal_akta',
        'alamat',
        'npwp',
        'bank',
        'norek'
    ];

    protected $casts = [
        'tanggal_akta' => 'date'
    ];

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('penyedia/dokumen');
    }
}
