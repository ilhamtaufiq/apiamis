<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\Permission\Models\Role;

class KegiatanRole extends Model
{
    protected $table = 'kegiatan_role';

    protected $fillable = [
        'role_id',
        'kegiatan_id'
    ];

    public function role(): BelongsTo
    {
        return $this->belongsTo(Role::class);
    }

    public function kegiatan(): BelongsTo
    {
        return $this->belongsTo(Kegiatan::class);
    }

    /**
     * Get kegiatan IDs untuk role tertentu
     */
    public static function getKegiatanIdsForRole($roleId)
    {
        return self::where('role_id', $roleId)->pluck('kegiatan_id');
    }
}
