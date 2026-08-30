<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class UserDriveShare extends Model
{
    protected $fillable = [
        'item_id',
        'shared_to_user_id',
    ];

    public function item(): BelongsTo
    {
        return $this->belongsTo(UserDriveItem::class, 'item_id');
    }

    public function sharedToUser(): BelongsTo
    {
        return $this->belongsTo(User::class, 'shared_to_user_id');
    }
}
