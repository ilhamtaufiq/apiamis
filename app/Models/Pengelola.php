<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class Pengelola extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
    protected $table = 'tbl_pengelola';

    protected $fillable = [
        'unit_spam_id',
        'pokmas',
        'perdes',
        'kepala',
        'bendahara',
        'sekretaris'
    ];

    public function unitSpam(): BelongsTo
    {
        return $this->belongsTo(UnitSpam::class, 'unit_spam_id');
    }
}
