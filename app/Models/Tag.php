<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsToMany;
use Illuminate\Support\Str;
use App\Traits\Auditable;

class Tag extends Model
{
    use Auditable;
    protected $table = 'tbl_tags';

    protected $fillable = [
        'name',
        'slug',
        'color',
    ];

    /**
     * Boot method to auto-generate slug
     */
    protected static function boot()
    {
        parent::boot();

        static::creating(function ($tag) {
            if (empty($tag->slug)) {
                $tag->slug = Str::slug($tag->name);
            }
        });

        static::updating(function ($tag) {
            if ($tag->isDirty('name') && !$tag->isDirty('slug')) {
                $tag->slug = Str::slug($tag->name);
            }
        });
    }

    /**
     * Pekerjaan yang memiliki tag ini
     */
    public function pekerjaan(): BelongsToMany
    {
        return $this->belongsToMany(Pekerjaan::class, 'pekerjaan_tag', 'tag_id', 'pekerjaan_id')
            ->withTimestamps();
    }
}
