<?php

namespace App\Models;

use App\Traits\Auditable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class PanduanPage extends Model
{
    use Auditable;

    protected $table = 'panduan_pages';

    protected $fillable = [
        'slug',
        'title',
        'description',
        'section',
        'sort_order',
        'body',
        'is_published',
        'updated_by',
    ];

    protected function casts(): array
    {
        return [
            'is_published' => 'boolean',
            'sort_order' => 'integer',
        ];
    }

    public function editor(): BelongsTo
    {
        return $this->belongsTo(User::class, 'updated_by');
    }

    public function scopePublished($query)
    {
        return $query->where('is_published', true);
    }

    public function scopeOrdered($query)
    {
        return $query->orderBy('section')->orderBy('sort_order')->orderBy('title');
    }
}
