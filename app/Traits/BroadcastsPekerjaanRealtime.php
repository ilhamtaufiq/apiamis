<?php

namespace App\Traits;

use App\Events\PekerjaanUpdated;
use Illuminate\Database\Eloquent\Model;

trait BroadcastsPekerjaanRealtime
{
    public static function bootBroadcastsPekerjaanRealtime(): void
    {
        static::created(function (Model $model) {
            static::broadcastPekerjaanUpdate($model, 'created');
        });

        static::updated(function (Model $model) {
            static::broadcastPekerjaanUpdate($model, 'updated');
        });

        static::deleted(function (Model $model) {
            static::broadcastPekerjaanUpdate($model, 'deleted');
        });
    }

    protected static function broadcastPekerjaanUpdate(Model $model, string $action): void
    {
        if (app()->runningInConsole()) {
            return;
        }

        if (config('broadcasting.default') === 'null') {
            return;
        }

        $pekerjaanId = static::resolvePekerjaanIdForBroadcast($model);
        if (! $pekerjaanId) {
            return;
        }

        $resource = strtolower(class_basename($model));
        $resourceId = $model->getKey();

        broadcast(new PekerjaanUpdated(
            pekerjaanId: (int) $pekerjaanId,
            resource: $resource,
            action: $action,
            resourceId: $resourceId ? (int) $resourceId : null,
        ));
    }

    protected static function resolvePekerjaanIdForBroadcast(Model $model): ?int
    {
        $modelName = class_basename($model);

        $pekerjaanId = match ($modelName) {
            'Pekerjaan' => $model->getKey(),
            'Kontrak' => $model->getAttribute('id_pekerjaan'),
            'Output', 'Penerima', 'Foto', 'Berkas', 'Progress' => $model->getAttribute('pekerjaan_id'),
            default => null,
        };

        return $pekerjaanId ? (int) $pekerjaanId : null;
    }
}