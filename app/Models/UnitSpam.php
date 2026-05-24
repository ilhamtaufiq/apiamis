<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasOne;
use Illuminate\Database\Eloquent\Relations\HasMany;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class UnitSpam extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
    protected $table = 'tbl_unit_spam';

    protected $fillable = [
        'desa_id',
        'name',
        'is_simspam',
        'sistem_layanan',
        'sumber_mata_air_kap',
        'sumber_air_tanah_kap',
        'lain_lain_kap'
    ];

    protected $casts = [
        'is_simspam' => 'boolean'
    ];

    public function desa(): BelongsTo
    {
        return $this->belongsTo(Desa::class, 'desa_id');
    }

    public function pengelola(): HasOne
    {
        return $this->hasOne(Pengelola::class, 'unit_spam_id');
    }

    public function checklists(): HasMany
    {
        return $this->hasMany(UnitChecklist::class, 'unit_spam_id');
    }

    public function achievements(): HasMany
    {
        return $this->hasMany(SpamAchievement::class, 'unit_spam_id');
    }

    public function budgets(): HasMany
    {
        return $this->hasMany(SpamBudget::class, 'unit_spam_id');
    }
}
