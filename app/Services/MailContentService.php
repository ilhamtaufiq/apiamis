<?php

namespace App\Services;

use Illuminate\Support\Str;

class MailContentService
{
    public const FORMAT_PLAIN = 'plain';

    public const FORMAT_MARKDOWN = 'markdown';

    public const FORMAT_HTML = 'html';

    /**
     * @return array{subject: string, body: string, format: string}
     */
    public static function resolveTestContent(?array $overrides = null): array
    {
        $overrides = $overrides ?? [];
        $templateKey = (string) ($overrides['template_key'] ?? 'smtp_test');
        $templateOverrides = array_filter([
            'format' => $overrides['format'] ?? $overrides['mail_body_format'] ?? null,
            'subject' => $overrides['subject'] ?? $overrides['mail_subject'] ?? null,
            'body' => $overrides['body'] ?? $overrides['mail_body'] ?? null,
        ], static fn ($value) => $value !== null && $value !== '');

        return MailTemplateService::resolve(
            $templateKey,
            [],
            $templateOverrides !== [] ? $templateOverrides : null
        );
    }

    /**
     * @param  array<string, string>  $variables
     */
    public static function applyPlaceholders(string $content, array $variables): string
    {
        $replaced = $content;

        foreach ($variables as $key => $value) {
            $replaced = str_replace('{{'.$key.'}}', (string) $value, $replaced);
        }

        return $replaced;
    }

    public static function defaultBody(string $format = self::FORMAT_MARKDOWN): string
    {
        return MailTemplateService::defaultBody('smtp_test', $format);
    }

    /**
     * @return array{html: ?string, text: string}
     */
    public static function render(string $body, string $format): array
    {
        $normalized = trim($body);
        $preheader = trim(strip_tags($normalized));

        if ($format === self::FORMAT_HTML) {
            $html = MailLayoutService::wrapDocument($normalized, $preheader);

            return [
                'html' => $html,
                'text' => trim(html_entity_decode(strip_tags($normalized))),
            ];
        }

        if ($format === self::FORMAT_MARKDOWN) {
            $innerHtml = (string) Str::markdown($normalized);
            $html = MailLayoutService::wrapDocument($innerHtml, $preheader);

            return [
                'html' => $html,
                'text' => trim(html_entity_decode(strip_tags($innerHtml))),
            ];
        }

        return [
            'html' => null,
            'text' => MailLayoutService::wrapPlainDocument($normalized),
        ];
    }

    public static function sendRendered(
        string $to,
        string $subject,
        string $body,
        string $format,
        ?string $recipientName = null,
    ): void {
        $rendered = self::render($body, $format);
        $toAddress = strtolower(trim($to));

        \Illuminate\Support\Facades\Mail::send([], [], function ($message) use (
            $toAddress,
            $recipientName,
            $subject,
            $rendered
        ) {
            $message->to($toAddress, $recipientName ?: null)->subject($subject);

            if ($rendered['html']) {
                $message->html($rendered['html']);
            }

            if ($rendered['text'] !== '') {
                $message->text($rendered['text']);
            }
        });
    }
}