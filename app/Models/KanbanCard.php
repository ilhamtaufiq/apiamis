<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class KanbanCard extends Model
{
    protected $table = 'tbl_kanban_cards';

    protected $fillable = [
        'board_id',
        'column_id',
        'position',
        'title',
        'description',
        'status_label',
        'pekerjaan_id',
        'tiket_id',
        'source',
        'metadata',
        'created_by',
    ];

    protected function casts(): array
    {
        return [
            'metadata' => 'array',
        ];
    }

    public function board(): BelongsTo
    {
        return $this->belongsTo(KanbanBoard::class, 'board_id');
    }

    public function column(): BelongsTo
    {
        return $this->belongsTo(KanbanColumn::class, 'column_id');
    }

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    public function tiket(): BelongsTo
    {
        return $this->belongsTo(Tiket::class, 'tiket_id');
    }

    public function creator(): BelongsTo
    {
        return $this->belongsTo(User::class, 'created_by');
    }
}