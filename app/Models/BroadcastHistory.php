<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class BroadcastHistory extends Model
{
    protected $fillable = [
        'title',
        'message',
        'type',
        'notification_type',
        'url',
        'is_banner',
        'recipient_count',
    ];
}
