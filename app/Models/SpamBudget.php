<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class SpamBudget extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
    protected $table = 'tbl_spam_budgets';

    protected $fillable = [
        'unit_spam_id',
        'nilai_kontrak',
        'tahun',
        'nama_paket',
        'sumber_dana'
    ];

    protected $casts = [
        'nilai_kontrak' => 'double',
        'unit_spam_id' => 'integer'
    ];

    public function unitSpam(): BelongsTo
    {
        return $this->belongsTo(UnitSpam::class, 'unit_spam_id');
    }
}
