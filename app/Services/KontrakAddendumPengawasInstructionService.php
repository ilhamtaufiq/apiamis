<?php

namespace App\Services;

use App\Models\Pekerjaan;
use App\Models\User;
use App\Notifications\AppNotification;
use Illuminate\Support\Facades\Log;

class KontrakAddendumPengawasInstructionService
{
    public function __construct(
        private readonly KontrakAddendumRegisterGapService $gapService,
    ) {}

    /**
     * @return array{
     *     message: string,
     *     notified_count: int,
     *     email_sent_count: int,
     *     recipients: list<array{user_id: int, name: string, email: ?string, notification_sent: bool, email_sent: bool, email_skipped_reason: ?string}>
     * }
     */
    public function notifyByRegisterId(int $registerId): array
    {
        $gaps = $this->gapService->findGaps();
        $gap = collect($gaps['items'])->firstWhere('register_id', $registerId);

        if (! is_array($gap)) {
            abort(404, 'Ketidaksesuaian register tidak ditemukan atau sudah dilengkapi');
        }

        return $this->notify($gap);
    }

    /**
     * @param  array<string, mixed>  $gap
     * @return array{
     *     message: string,
     *     notified_count: int,
     *     email_sent_count: int,
     *     recipients: list<array{user_id: int, name: string, email: ?string, notification_sent: bool, email_sent: bool, email_skipped_reason: ?string}>
     * }
     */
    public function notify(array $gap): array
    {
        $pekerjaanId = data_get($gap, 'pekerjaan.id');

        if (! $pekerjaanId) {
            abort(422, 'Pekerjaan tidak ditemukan untuk register ini');
        }

        $pekerjaan = Pekerjaan::query()
            ->with('assignedUsers')
            ->findOrFail($pekerjaanId);

        $recipients = $pekerjaan->assignedUsers
            ->filter(fn (User $user) => $user->id)
            ->unique('id')
            ->values();

        if ($recipients->isEmpty()) {
            abort(422, 'Tidak ada pengawas yang di-assign ke pekerjaan ini');
        }

        $title = 'Instruksi: Lengkapi Data Addendum Kontrak';
        $actionUrl = FrontendUrlService::pengawasApp("pekerjaan/{$pekerjaanId}");

        $recipientResults = [];

        foreach ($recipients as $recipient) {
            $message = $this->buildInstructionMessage($gap, $recipient->name);
            $recipient->notify(new AppNotification($title, $message, $actionUrl, 'warning'));

            $emailResult = $this->sendInstructionEmail($recipient, $title, $message, $actionUrl);

            $recipientResults[] = [
                'user_id' => $recipient->id,
                'name' => $recipient->name,
                'email' => $emailResult['email'],
                'notification_sent' => true,
                'email_sent' => $emailResult['sent'],
                'email_skipped_reason' => $emailResult['skipped_reason'],
            ];
        }

        $emailSentCount = collect($recipientResults)->where('email_sent', true)->count();

        return [
            'message' => 'Instruksi pengawas berhasil dikirim',
            'notified_count' => count($recipientResults),
            'email_sent_count' => $emailSentCount,
            'recipients' => $recipientResults,
        ];
    }

    /**
     * @param  array<string, mixed>  $gap
     */
    public function buildInstructionMessage(array $gap, ?string $recipientName = null): string
    {
        $pekerjaan = (string) data_get($gap, 'pekerjaan.nama_paket', 'pekerjaan terkait');
        $nomor = (string) data_get($gap, 'nomor_register', '-');
        $tanggal = $this->formatDate(data_get($gap, 'tanggal_register'));
        $pengawas = trim((string) ($recipientName ?: data_get($gap, 'pengawas.nama', 'Pengawas')));

        if ($pengawas === '') {
            $pengawas = 'Pengawas';
        }

        return implode("\n", [
            "Yth. {$pengawas},",
            '',
            "Mohon dilengkapi data addendum kontrak untuk pekerjaan \"{$pekerjaan}\".",
            '',
            "Nomor register addendum sudah dibuat: {$nomor} ({$tanggal}).",
            'Detail addendum (data pengajuan, lampiran, dan nilai) belum ada di sistem.',
            'Status persetujuan: belum disetujui.',
            '',
            'Silakan buat/melengkapi pengajuan addendum dengan nomor register yang sama.',
        ]);
    }

    /**
     * @return array{sent: bool, skipped_reason: ?string, email: ?string}
     */
    private function sendInstructionEmail(User $recipient, string $title, string $message, string $actionUrl): array
    {
        $email = strtolower(trim((string) $recipient->email));

        if ($email === '' || ! filter_var($email, FILTER_VALIDATE_EMAIL)) {
            return ['sent' => false, 'skipped_reason' => 'no_email', 'email' => null];
        }

        if (! MailConfigService::applyFromSettings()) {
            return ['sent' => false, 'skipped_reason' => 'smtp_disabled', 'email' => $email];
        }

        try {
            $content = MailTemplateService::renderDeliverable('broadcast', [
                'title' => $title,
                'message' => $message,
                'action_url' => $actionUrl,
            ]);

            MailContentService::sendRendered(
                $email,
                $content['subject'],
                $content['body'],
                $content['format'],
                $recipient->name ?: null,
            );

            return ['sent' => true, 'skipped_reason' => null, 'email' => $email];
        } catch (\Throwable $e) {
            Log::warning('Gagal mengirim email instruksi addendum ke pengawas', [
                'user_id' => $recipient->id,
                'email' => $email,
                'error' => $e->getMessage(),
            ]);

            return ['sent' => false, 'skipped_reason' => 'send_failed', 'email' => $email];
        }
    }

    private function formatDate(mixed $value): string
    {
        if (! $value) {
            return '-';
        }

        try {
            return \Carbon\Carbon::parse($value)->locale('id')->translatedFormat('d M Y');
        } catch (\Throwable) {
            return (string) $value;
        }
    }
}