<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;

class KanbanColumn extends Model
{
    protected $table = 'tbl_kanban_columns';

    protected $fillable = [
        'board_id',
        'title',
        'position',
        'tiket_status',
        'color',
    ];

    public function board(): BelongsTo
    {
        return $this->belongsTo(KanbanBoard::class, 'board_id');
    }

    public function cards(): HasMany
    {
        return $this->hasMany(KanbanCard::class, 'column_id')->orderBy('position');
    }
}