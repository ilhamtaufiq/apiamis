<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class PuspenReviewNote extends Model
{
    use HasFactory;

    protected $fillable = [
        'pekerjaan_id',
        'user_id',
        'content',
    ];

    protected $casts = [
        'pekerjaan_id' => 'integer',
        'user_id' => 'integer',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class, 'user_id');
    }
}