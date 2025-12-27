<?php

namespace App\Traits;

use App\Models\AuditLog;
use Illuminate\Support\Facades\Request;

trait Auditable
{
    public static function bootAuditable()
    {
        static::created(function ($model) {
            $model->logAudit('created');
        });

        static::updated(function ($model) {
            $model->logAudit('updated');
        });

        static::deleted(function ($model) {
            $model->logAudit('deleted');
        });
    }

    protected function logAudit($event)
    {
        // Don't log if running in console (seeders, commands) unless explicitly needed
        if (app()->runningInConsole()) {
            return;
        }

        $oldValues = $event === 'updated' ? array_intersect_key($this->getRawOriginal(), $this->getDirty()) : null;
        $newValues = $event === 'updated' ? $this->getDirty() : ($event === 'created' ? $this->getAttributes() : null);

        // For deleted, we might want to store the whole model state as old_values
        if ($event === 'deleted') {
            $oldValues = $this->getAttributes();
            $newValues = null;
        }

        // Filter out sensitive fields or large blobs if necessary
        $exclude = ['password', 'remember_token'];
        if ($oldValues) {
            $oldValues = array_diff_key($oldValues, array_flip($exclude));
        }
        if ($newValues) {
            $newValues = array_diff_key($newValues, array_flip($exclude));
        }

        AuditLog::create([
            'user_id' => auth()->id(),
            'event' => $event,
            'auditable_type' => get_class($this),
            'auditable_id' => $this->id,
            'old_values' => $oldValues,
            'new_values' => $newValues,
            'url' => Request::fullUrl(),
            'ip_address' => Request::ip(),
            'user_agent' => Request::userAgent(),
        ]);
    }
}
