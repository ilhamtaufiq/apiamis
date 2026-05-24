<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class UnitChecklist extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
    protected $table = 'tbl_unit_checklists';

    protected $fillable = [
        'unit_spam_id',
        'item',
        'is_checked'
    ];

    protected $casts = [
        'is_checked' => 'boolean'
    ];

    public function unitSpam(): BelongsTo
    {
        return $this->belongsTo(UnitSpam::class, 'unit_spam_id');
    }
}
