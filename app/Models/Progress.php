<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class Progress extends Model
{
    use HasFactory, NotifiesAdminsOnChanges, Auditable;

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
