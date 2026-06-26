<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;

class KanbanBoard extends Model
{
    protected $table = 'tbl_kanban_boards';

    protected $fillable = [
        'slug',
        'title',
        'description',
    ];

    public function columns(): HasMany
    {
        return $this->hasMany(KanbanColumn::class, 'board_id')->orderBy('position');
    }

    public function cards(): HasMany
    {
        return $this->hasMany(KanbanCard::class, 'board_id')->orderBy('position');
    }
}