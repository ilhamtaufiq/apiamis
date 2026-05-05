<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsToMany;
use App\Traits\Auditable;

class ChecklistItem extends Model
{
    use Auditable;
    protected $table = 'tbl_checklist_items';

    protected $fillable = [
        'name',
        'description',
        'sort_order',
    ];

    protected $casts = [
        'sort_order' => 'integer',
    ];

    /**
     * Pekerjaan yang memiliki checklist item ini
     */
    public function pekerjaan(): BelongsToMany
    {
        return $this->belongsToMany(Pekerjaan::class, 'pekerjaan_checklist', 'checklist_item_id', 'pekerjaan_id')
            ->withPivot(['is_checked', 'checked_at', 'checked_by', 'notes'])
            ->withTimestamps();
    }
}
