<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class PekerjaanChecklistHistory extends Model
{
    public $timestamps = false;

    protected $table = 'pekerjaan_checklist_histories';

    protected $fillable = [
        'pekerjaan_id',
        'checklist_item_id',
        'is_checked',
        'notes',
        'user_id',
        'created_at',
    ];

    protected $casts = [
        'is_checked' => 'boolean',
        'created_at' => 'datetime',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    public function checklistItem(): BelongsTo
    {
        return $this->belongsTo(ChecklistItem::class, 'checklist_item_id');
    }

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class, 'user_id');
    }
}
