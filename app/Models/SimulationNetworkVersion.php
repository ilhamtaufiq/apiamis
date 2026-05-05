<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use App\Traits\Auditable;

class SimulationNetworkVersion extends Model
{
    use Auditable;
    protected $table = 'simulation_network_versions';

    public $timestamps = false;

    protected $fillable = [
        'simulation_network_id',
        'version',
        'network_data',
        'simulation_settings',
        'change_description',
        'changed_by',
        'created_at',
    ];

    protected $casts = [
        'network_data' => 'array',
        'simulation_settings' => 'array',
        'version' => 'integer',
        'created_at' => 'datetime',
    ];

    protected static function boot()
    {
        parent::boot();

        static::creating(function ($model) {
            $model->created_at = $model->created_at ?? now();
        });
    }

    /**
     * The network this version belongs to
     */
    public function network(): BelongsTo
    {
        return $this->belongsTo(SimulationNetwork::class, 'simulation_network_id');
    }

    /**
     * User who made this change
     */
    public function changedBy(): BelongsTo
    {
        return $this->belongsTo(User::class, 'changed_by');
    }
}
