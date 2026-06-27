<?php

namespace App\Models;

use App\Traits\Auditable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\SoftDeletes;

class BlogComment extends Model
{
    use Auditable, SoftDeletes;

    public const MAX_DEPTH = 10;

    protected $table = 'tbl_blog_comment';

    protected $fillable = [
        'blog_id',
        'user_id',
        'parent_id',
        'body',
        'depth',
    ];

    protected $casts = [
        'depth' => 'integer',
    ];

    public function blog(): BelongsTo
    {
        return $this->belongsTo(Blog::class, 'blog_id');
    }

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    public function parent(): BelongsTo
    {
        return $this->belongsTo(self::class, 'parent_id');
    }

    public function replies(): HasMany
    {
        return $this->hasMany(self::class, 'parent_id')->orderBy('created_at');
    }
}