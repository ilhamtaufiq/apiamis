<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;

use App\Traits\Auditable;
use App\Traits\BroadcastsPekerjaanRealtime;
use App\Traits\NotifiesAdminsOnChanges;

class Progress extends Model
{
    use BroadcastsPekerjaanRealtime, HasFactory, NotifiesAdminsOnChanges, Auditable;

    protected $table = 'tbl_progress';

    protected $fillable = [
        'pekerjaan_id',
        'content',
    ];

    protected $casts = [
        'content' => 'array',
    ];

    public function pekerjaan()
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }
}
