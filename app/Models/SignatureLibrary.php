<?php

namespace App\Models;

use App\Traits\Auditable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\SoftDeletes;

class SignatureLibrary extends Model
{
    use SoftDeletes, Auditable;

    protected $table = 'signature_libraries';

    protected $fillable = [
        'user_id',
        'name',
        'mime_type',
        'data_url',
        'width',
        'height',
    ];

    protected $casts = [
        'width' => 'integer',
        'height' => 'integer',
    ];

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class, 'user_id');
    }

    public function scopeOwnedBy($query, int $userId)
    {
        return $query->where('user_id', $userId);
    }

    public function canManage(User $user): bool
    {
        return $this->user_id === $user->id || $user->hasRole('admin');
    }
}
