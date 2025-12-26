<?php

namespace App\Traits;

use App\Models\User;
use App\Notifications\AppNotification;
use Illuminate\Support\Facades\Log;

trait NotifiesAdminsOnChanges
{
    public static function bootNotifiesAdminsOnChanges()
    {
        static::created(function ($model) {
            static::notifyAdmins($model, 'dibuat');
        });

        static::updated(function ($model) {
            static::notifyAdmins($model, 'diperbarui');
        });

        static::deleted(function ($model) {
            static::notifyAdmins($model, 'dihapus');
        });
    }

    protected static function notifyAdmins($model, $action)
    {
        // Don't notify if the change was made by a seeder or console command
        if (app()->runningInConsole()) {
            return;
        }

        $user = auth()->user();
        $userName = $user ? $user->name : 'System';
        
        $modelName = class_basename($model);
        $title = "Data $modelName $action";
        $message = "Model $modelName dengan ID #{$model->id} telah $action oleh $userName.";
        
        // Construct a URL for the notification if possible
        $url = null;
        if ($modelName === 'Pekerjaan') {
            $url = "/pekerjaan/{$model->id}";
        } elseif ($modelName === 'Tiket') {
            $url = "/tiket/{$model->id}";
        }

        try {
            $admins = User::role('admin')->get();
            foreach ($admins as $admin) {
                // Skip notifying the person who made the change if they are an admin
                if ($user && $admin->id === $user->id) {
                    continue;
                }
                $admin->notify(new AppNotification($title, $message, $url, 'info'));
            }
        } catch (\Exception $e) {
            Log::error("Failed to notify admins about $modelName $action: " . $e->getMessage());
        }
    }
}
