<?php

namespace App\Services;

use App\Models\AppSetting;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Mail;

class ContactInquiryService
{
    /**
     * @param  array{name: string, email: string, phone?: string, subject: string, message: string}  $payload
     * @return array{sent: bool, error?: string}
     */
    public function send(array $payload): array
    {
        if (! MailConfigService::applyFromSettings()) {
            return [
                'sent' => false,
                'error' => 'Layanan email belum diaktifkan. Silakan hubungi kami melalui Instagram atau datang langsung ke kantor.',
            ];
        }

        $recipient = $this->resolveRecipientEmail();
        if ($recipient === null) {
            return [
                'sent' => false,
                'error' => 'Email tujuan hubungi kami belum dikonfigurasi di pengaturan aplikasi.',
            ];
        }

        $appName = (string) AppSetting::getValue('app_name', 'Arumanis');
        $name = trim($payload['name']);
        $email = strtolower(trim($payload['email']));
        $phone = trim((string) ($payload['phone'] ?? ''));
        $subject = trim($payload['subject']);
        $message = trim($payload['message']);

        $mailSubject = '[Hubungi Kami] '.$subject.' — '.$appName;
        $rendered = $this->buildMailContent($name, $email, $phone, $subject, $message);

        try {
            Mail::send([], [], function ($mail) use (
                $recipient,
                $mailSubject,
                $rendered,
                $email,
                $name,
            ) {
                $mail->to($recipient)->subject($mailSubject);

                if ($rendered['html']) {
                    $mail->html($rendered['html']);
                }

                if ($rendered['text'] !== '') {
                    $mail->text($rendered['text']);
                }

                $mail->replyTo($email, $name !== '' ? $name : null);
            });

            return ['sent' => true];
        } catch (\Throwable $e) {
            Log::warning('Gagal mengirim pesan hubungi kami', [
                'recipient' => $recipient,
                'sender_email' => $email,
                'error' => $e->getMessage(),
            ]);

            return [
                'sent' => false,
                'error' => 'Pesan tidak dapat dikirim saat ini. Silakan coba lagi nanti.',
            ];
        }
    }

    public function resolveRecipientEmail(): ?string
    {
        foreach ([
            AppSetting::getValue('contact_email'),
            AppSetting::getValue('mail_from_address'),
            AppSetting::getValue('mail_username'),
        ] as $candidate) {
            $normalized = strtolower(trim((string) $candidate));
            if ($normalized !== '' && filter_var($normalized, FILTER_VALIDATE_EMAIL)) {
                return $normalized;
            }
        }

        return null;
    }

    /**
     * @return array{html: ?string, text: string}
     */
    private function buildMailContent(
        string $name,
        string $email,
        string $phone,
        string $subject,
        string $message,
    ): array {
        $safeName = htmlspecialchars($name, ENT_QUOTES, 'UTF-8');
        $safeEmail = htmlspecialchars($email, ENT_QUOTES, 'UTF-8');
        $safePhone = htmlspecialchars($phone, ENT_QUOTES, 'UTF-8');
        $safeSubject = htmlspecialchars($subject, ENT_QUOTES, 'UTF-8');

        $contactLines = '<strong>Nama:</strong> '.$safeName.'<br>'
            .'<strong>Email:</strong> <a href="mailto:'.$safeEmail.'">'.$safeEmail.'</a>';
        if ($phone !== '') {
            $contactLines .= '<br><strong>Telepon:</strong> '.$safePhone;
        }
        $contactLines .= '<br><strong>Subjek:</strong> '.$safeSubject;

        $inner = MailLayoutService::heading('Pesan Hubungi Kami', 'Formulir landing page '.$safeSubject)
            .MailLayoutService::infoBox($contactLines)
            .MailLayoutService::paragraph('Pesan:')
            .MailLayoutService::messageBlock($message);

        $html = MailLayoutService::wrapDocument($inner, $message);

        $plain = implode("\n", array_filter([
            'Pesan Hubungi Kami',
            'Nama: '.$name,
            'Email: '.$email,
            $phone !== '' ? 'Telepon: '.$phone : null,
            'Subjek: '.$subject,
            '',
            $message,
        ]));

        return [
            'html' => $html,
            'text' => MailLayoutService::wrapPlainDocument($plain),
        ];
    }
}