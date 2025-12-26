<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class Event extends Model
{
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
    ];

    protected $casts = [
        'is_allday' => 'boolean',
        'start' => 'datetime',
        'end' => 'datetime',
    ];

    public function user()
    {
        return $this->belongsTo(User::class);
    }
}
