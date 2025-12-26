<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;

use App\Traits\NotifiesAdminsOnChanges;

class Kecamatan extends Model
{
    use NotifiesAdminsOnChanges;
    protected $table = 'tbl_kecamatan';
    
    protected $fillable = [
        'n_kec'
    ];

    /**
     * Relasi One-to-Many dengan Desa
     */
    public function desa(): HasMany
    {
        return $this->hasMany(Desa::class, 'kecamatan_id');
    }
}
