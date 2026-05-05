<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class Output extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
    protected $table = 'tbl_output';

    protected $fillable = [
        'pekerjaan_id',
        'komponen',
        'satuan',
        'volume',
        'penerima_is_optional'
    ];

    protected $casts = [
        'pekerjaan_id' => 'integer',
        'volume' => 'decimal:2',
        'penerima_is_optional' => 'boolean',
    ];

    /**
     * Relasi Many-to-One dengan Pekerjaan
     */
    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }
}
