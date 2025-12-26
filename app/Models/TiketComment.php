<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class TiketComment extends Model
{
    use HasFactory;

    protected $table = 'tbl_tiket_comment';

    protected $fillable = [
        'tiket_id',
        'user_id',
        'message',
    ];

    /**
     * Get the ticket that owns the comment.
     */
    public function tiket(): BelongsTo
    {
        return $this->belongsTo(Tiket::class);
    }

    /**
     * Get the user that wrote the comment.
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }
}
