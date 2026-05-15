<?php

namespace App\Notifications;

use Illuminate\Bus\Queueable;
use Illuminate\Contracts\Queue\ShouldQueue;
use Illuminate\Notifications\Messages\MailMessage;
use Illuminate\Notifications\Notification;

class AppNotification extends Notification
{
    use Queueable;

    protected $title;
    protected $message;
    protected $url;
    protected $type;
    protected $isBanner;
    protected $broadcastHistoryId;

    /**
     * Create a new notification instance.
     */
    public function __construct($title, $message, $url = null, $type = 'info', $isBanner = false, $broadcastHistoryId = null)
    {
        $this->title = $title;
        $this->message = $message;
        $this->url = $url;
        $this->type = $type;
        $this->isBanner = $isBanner;
        $this->broadcastHistoryId = $broadcastHistoryId;
    }

    /**
     * Get the notification's delivery channels.
     *
     * @return array<int, string>
     */
    public function via(object $notifiable): array
    {
        return ['database'];
    }

    /**
     * Get the array representation of the notification.
     *
     * @return array<string, mixed>
     */
    public function toArray(object $notifiable): array
    {
        return [
            'title' => $this->title,
            'message' => $this->message,
            'url' => $this->url,
            'type' => $this->type,
            'is_banner' => $this->isBanner,
            'broadcast_history_id' => $this->broadcastHistoryId,
        ];
    }
}
