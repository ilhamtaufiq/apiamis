<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use App\Traits\Auditable;

class Blog extends Model
{
    use Auditable;

    protected $table = 'tbl_blog';

    /**
     * Get the route key for the model.
     */
    public function getRouteKeyName()
    {
        return 'slug';
    }

    protected $fillable = [
        'title',
        'slug',
        'content',
        'category',
        'cover_image',
        'user_id',
        'is_published',
        'is_internal',
        'is_featured',
        'published_at',
        'featured_at',
    ];

    protected $casts = [
        'is_published' => 'boolean',
        'is_internal' => 'boolean',
        'is_featured' => 'boolean',
        'published_at' => 'datetime',
        'featured_at' => 'datetime',
    ];

    /**
     * Get the user that authored the blog post.
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class, 'user_id');
    }
}

