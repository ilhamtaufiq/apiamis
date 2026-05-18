<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class BlogAsset extends Model implements HasMedia
{
    use InteractsWithMedia;

    protected $table = 'tbl_blog_assets';

    protected $fillable = [
        'user_id',
        'blog_id',
    ];

    public function blog(): BelongsTo
    {
        return $this->belongsTo(Blog::class, 'blog_id');
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('blog/videos');
    }
}
