<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class Tiket extends Model implements HasMedia
{
    use HasFactory, InteractsWithMedia;

    protected $table = 'tbl_tiket';

    protected $fillable = [
        'user_id',
        'pekerjaan_id',
        'subjek',
        'deskripsi',
        'kategori',
        'prioritas',
        'status',
        'admin_notes',
    ];

    /**
     * Get the user that owns the ticket.
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    /**
     * Get the pekerjaan associated with the ticket.
     */
    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }
    /**
     * Get the comments for the ticket.
     */
    public function comments()
    {
        return $this->hasMany(TiketComment::class, 'tiket_id');
    }
}
