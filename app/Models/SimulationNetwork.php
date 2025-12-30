<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\SoftDeletes;
use App\Traits\Auditable;

class SimulationNetwork extends Model
{
    use SoftDeletes, Auditable;

    protected $table = 'simulation_networks';

    protected $fillable = [
        'name',
        'description',
        'user_id',
        'pekerjaan_id',
        'network_data',
        'simulation_settings',
        'last_results',
        'last_simulated_at',
        'version',
        'is_public',
    ];

    protected $casts = [
        'network_data' => 'array',
        'simulation_settings' => 'array',
        'last_results' => 'array',
        'last_simulated_at' => 'datetime',
        'version' => 'integer',
        'is_public' => 'boolean',
    ];

    /**
     * Default simulation settings
     */
    public static function defaultSettings(): array
    {
        return [
            'duration' => 24,           // hours
            'hydraulic_timestep' => 1,  // hours
            'pattern_timestep' => 1,    // hours
            'report_timestep' => 1,     // hours
            'units' => 'LPS',           // Liters per second
            'headloss' => 'H-W',        // Hazen-Williams
        ];
    }

    /**
     * Owner of this network
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class, 'user_id');
    }

    /**
     * Optional link to pekerjaan (infrastructure project)
     */
    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    /**
     * Version history for this network
     */
    public function versions(): HasMany
    {
        return $this->hasMany(SimulationNetworkVersion::class, 'simulation_network_id')
            ->orderBy('version', 'desc');
    }

    /**
     * Scope: networks owned by a specific user
     */
    public function scopeOwnedBy($query, $userId)
    {
        return $query->where('user_id', $userId);
    }

    /**
     * Scope: networks accessible by a user (owned or public)
     */
    public function scopeAccessibleBy($query, $userId)
    {
        return $query->where(function ($q) use ($userId) {
            $q->where('user_id', $userId)
              ->orWhere('is_public', true);
        });
    }

    /**
     * Scope: networks linked to a pekerjaan
     */
    public function scopeForPekerjaan($query, $pekerjaanId)
    {
        return $query->where('pekerjaan_id', $pekerjaanId);
    }

    /**
     * Check if user can edit this network
     */
    public function canEdit(User $user): bool
    {
        // Owner can always edit
        if ($this->user_id === $user->id) {
            return true;
        }

        // Admin can edit all
        if ($user->hasRole('admin')) {
            return true;
        }

        return false;
    }

    /**
     * Check if user can view this network
     */
    public function canView(User $user): bool
    {
        // Public networks are viewable by all
        if ($this->is_public) {
            return true;
        }

        // Owner can view
        if ($this->user_id === $user->id) {
            return true;
        }

        // Admin can view all
        if ($user->hasRole('admin')) {
            return true;
        }

        return false;
    }

    /**
     * Save a new version before updating
     */
    public function saveVersion(string $description = null): void
    {
        SimulationNetworkVersion::create([
            'simulation_network_id' => $this->id,
            'version' => $this->version,
            'network_data' => $this->network_data,
            'simulation_settings' => $this->simulation_settings,
            'change_description' => $description,
            'changed_by' => auth()->id(),
        ]);

        $this->increment('version');
    }

    /**
     * Restore to a specific version
     */
    public function restoreToVersion(int $version): bool
    {
        $versionRecord = $this->versions()
            ->where('version', $version)
            ->first();

        if (!$versionRecord) {
            return false;
        }

        // Save current state as new version before restore
        $this->saveVersion("Restored to version {$version}");

        // Restore the old data
        $this->update([
            'network_data' => $versionRecord->network_data,
            'simulation_settings' => $versionRecord->simulation_settings,
        ]);

        return true;
    }

    /**
     * Get network statistics
     */
    public function getStatsAttribute(): array
    {
        $data = $this->network_data ?? [];

        return [
            'junctions' => count($data['junctions'] ?? []),
            'reservoirs' => count($data['reservoirs'] ?? []),
            'tanks' => count($data['tanks'] ?? []),
            'pipes' => count($data['pipes'] ?? []),
            'pumps' => count($data['pumps'] ?? []),
            'valves' => count($data['valves'] ?? []),
            'total_nodes' => count($data['junctions'] ?? []) + count($data['reservoirs'] ?? []) + count($data['tanks'] ?? []),
            'total_links' => count($data['pipes'] ?? []) + count($data['pumps'] ?? []) + count($data['valves'] ?? []),
        ];
    }
}
