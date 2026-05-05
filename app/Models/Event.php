<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;
use App\Traits\Auditable;

class Event extends Model implements HasMedia
{
    use InteractsWithMedia, Auditable;

    protected $table = 'tbl_events';

    protected $fillable = [
        'user_id',
        'title',
        'is_allday',
        'start',
        'end',
        'category',
        'location',
        'description',
        'color',
        'bg_color',
        'border_color',
        'attachments',
    ];

    protected $casts = [
        'is_allday' => 'boolean',
        'start' => 'datetime',
        'end' => 'datetime',
        'attachments' => 'array',
    ];

    public function user()
    {
        return $this->belongsTo(User::class);
    }
}
