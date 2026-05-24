<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class SpamAchievement extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
    protected $table = 'tbl_spam_achievements';

    protected $fillable = [
        'unit_spam_id',
        'tahun',
        'jumlah_sr',
        'jumlah_kk',
        'jumlah_jiwa',
        'jumlah_bjp_kk',
        'jumlah_bjp_jiwa',
        'catatan'
    ];

    protected $casts = [
        'jumlah_sr' => 'integer',
        'jumlah_kk' => 'integer',
        'jumlah_jiwa' => 'integer',
        'jumlah_bjp_kk' => 'integer',
        'jumlah_bjp_jiwa' => 'integer'
    ];

    public function unitSpam(): BelongsTo
    {
        return $this->belongsTo(UnitSpam::class, 'unit_spam_id');
    }
}
