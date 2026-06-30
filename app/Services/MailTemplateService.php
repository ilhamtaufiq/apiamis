<?php

namespace App\Services;

use App\Models\AppSetting;

class MailTemplateService
{
    public const STORAGE_KEY = 'mail_templates';

    /**
     * @var array<string, array{
     *     label: string,
     *     description: string,
     *     category: string,
     *     placeholders: list<string>,
     *     default_format: string
     * }>
     */
    public const TEMPLATES = [
        'smtp_test' => [
            'label' => 'Uji Koneksi SMTP',
            'description' => 'Email uji saat mengkonfigurasi atau memverifikasi SMTP.',
            'category' => 'sistem',
            'placeholders' => ['app_name'],
            'default_format' => MailContentService::FORMAT_HTML,
        ],
        'forgot_password' => [
            'label' => 'Lupa Password',
            'description' => 'Email reset password saat pengguna meminta pemulihan akses.',
            'category' => 'autentikasi',
            'placeholders' => ['user_name', 'user_email', 'reset_link', 'expiry_minutes', 'app_name'],
            'default_format' => MailContentService::FORMAT_HTML,
        ],
        'welcome' => [
            'label' => 'Selamat Datang',
            'description' => 'Email sambutan untuk pengguna baru yang terdaftar.',
            'category' => 'autentikasi',
            'placeholders' => ['user_name', 'user_email', 'login_url', 'app_name'],
            'default_format' => MailContentService::FORMAT_HTML,
        ],
        'broadcast' => [
            'label' => 'Notifikasi Broadcast',
            'description' => 'Pengumuman massal ke banyak pengguna sekaligus.',
            'category' => 'notifikasi',
            'placeholders' => ['title', 'message', 'action_url', 'app_name'],
            'default_format' => MailContentService::FORMAT_HTML,
        ],
        'ticket_created' => [
            'label' => 'Tiket Baru',
            'description' => 'Notifikasi saat tiket bantuan baru dibuat.',
            'category' => 'notifikasi',
            'placeholders' => ['user_name', 'ticket_id', 'ticket_title', 'ticket_url', 'app_name'],
            'default_format' => MailContentService::FORMAT_HTML,
        ],
        'ticket_updated' => [
            'label' => 'Update Tiket',
            'description' => 'Notifikasi perubahan status atau balasan tiket.',
            'category' => 'notifikasi',
            'placeholders' => ['user_name', 'ticket_id', 'ticket_title', 'status', 'ticket_url', 'app_name'],
            'default_format' => MailContentService::FORMAT_HTML,
        ],
        'task_assigned' => [
            'label' => 'Penugasan Pekerjaan',
            'description' => 'Pemberitahuan saat pengguna ditugaskan pada pekerjaan.',
            'category' => 'operasional',
            'placeholders' => ['user_name', 'pekerjaan_name', 'pekerjaan_url', 'app_name'],
            'default_format' => MailContentService::FORMAT_HTML,
        ],
        'data_reminder' => [
            'label' => 'Pengingat Kelengkapan Data',
            'description' => 'Reminder profil atau data pekerjaan yang belum lengkap.',
            'category' => 'operasional',
            'placeholders' => ['user_name', 'missing_fields', 'profile_url', 'app_name'],
            'default_format' => MailContentService::FORMAT_HTML,
        ],
        'contract_ready' => [
            'label' => 'Dokumen Kontrak Siap',
            'description' => 'Informasi dokumen kontrak yang siap diunduh atau ditinjau.',
            'category' => 'operasional',
            'placeholders' => ['user_name', 'kontrak_name', 'download_url', 'app_name'],
            'default_format' => MailContentService::FORMAT_HTML,
        ],
        'report_submitted' => [
            'label' => 'Laporan Dikirim',
            'description' => 'Konfirmasi laporan pekerjaan berhasil dikirim.',
            'category' => 'operasional',
            'placeholders' => ['user_name', 'report_name', 'report_url', 'app_name'],
            'default_format' => MailContentService::FORMAT_HTML,
        ],
    ];

    public static function isValidKey(string $key): bool
    {
        return isset(self::TEMPLATES[$key]);
    }

    /**
     * @return list<array{
     *     key: string,
     *     label: string,
     *     description: string,
     *     category: string,
     *     placeholders: list<string>,
     *     format: string,
     *     subject: string,
     *     body: string,
     *     is_custom: bool,
     *     updated_at: ?string,
     *     presets: array<string, array{subject: string, body: string}>
     * }>
     */
    public static function catalog(): array
    {
        $stored = self::storedMap();
        $legacy = self::legacySmtpTestOverride();
        $updatedAt = self::storageUpdatedAt();

        return collect(self::TEMPLATES)
            ->map(function (array $definition, string $key) use ($stored, $legacy, $updatedAt) {
                $custom = $stored[$key] ?? null;

                if ($key === 'smtp_test' && $custom === null && $legacy !== null) {
                    $custom = $legacy;
                }

                $format = (string) ($custom['format'] ?? $definition['default_format']);
                $subject = (string) ($custom['subject'] ?? self::defaultSubject($key));
                $body = (string) ($custom['body'] ?? self::defaultBody($key, $format));

                return [
                    'key' => $key,
                    'label' => $definition['label'],
                    'description' => $definition['description'],
                    'category' => $definition['category'],
                    'placeholders' => $definition['placeholders'],
                    'format' => $format,
                    'subject' => $subject,
                    'body' => $body,
                    'is_custom' => $custom !== null,
                    'updated_at' => $custom !== null ? $updatedAt : null,
                    'presets' => self::presetsFor($key),
                ];
            })
            ->values()
            ->all();
    }

    /**
     * @param  array<string, array{format?: string, subject?: string, body?: string}>  $templates
     */
    public static function saveMany(array $templates): void
    {
        $stored = self::storedMap();

        foreach ($templates as $key => $payload) {
            if (! self::isValidKey($key) || ! is_array($payload)) {
                continue;
            }

            $format = (string) ($payload['format'] ?? self::TEMPLATES[$key]['default_format']);
            if (! in_array($format, [MailContentService::FORMAT_PLAIN, MailContentService::FORMAT_MARKDOWN, MailContentService::FORMAT_HTML], true)) {
                $format = self::TEMPLATES[$key]['default_format'];
            }

            $subject = trim((string) ($payload['subject'] ?? ''));
            $body = (string) ($payload['body'] ?? '');

            $stored[$key] = [
                'format' => $format,
                'subject' => $subject !== '' ? $subject : self::defaultSubject($key),
                'body' => $body !== '' ? $body : self::defaultBody($key, $format),
            ];
        }

        AppSetting::setValue(self::STORAGE_KEY, json_encode($stored, JSON_UNESCAPED_UNICODE), 'text');
    }

    /**
     * @param  array<string, string>  $variables
     * @return array{subject: string, body: string, format: string}
     */
    public static function resolve(string $key, array $variables = [], ?array $overrides = null): array
    {
        if (! self::isValidKey($key)) {
            throw new \InvalidArgumentException("Unknown mail template key: {$key}");
        }

        $catalog = collect(self::catalog())->firstWhere('key', $key);
        $format = (string) ($overrides['format'] ?? $catalog['format'] ?? self::TEMPLATES[$key]['default_format']);
        $subject = (string) ($overrides['subject'] ?? $catalog['subject'] ?? self::defaultSubject($key));
        $body = (string) ($overrides['body'] ?? $catalog['body'] ?? self::defaultBody($key, $format));

        $vars = self::sampleVariables($key, $variables);

        return [
            'subject' => MailContentService::applyPlaceholders($subject, $vars),
            'body' => MailContentService::applyPlaceholders($body, $vars),
            'format' => $format,
        ];
    }

    /**
     * Render email for system delivery. Uses stored subject/format when customized,
     * but always builds body from the canonical default template so placeholders
     * like {{action_url}} are never replaced by stale sample URLs.
     *
     * @param  array<string, string>  $variables
     * @return array{subject: string, body: string, format: string}
     */
    public static function renderDeliverable(string $key, array $variables = []): array
    {
        if (! self::isValidKey($key)) {
            throw new \InvalidArgumentException("Unknown mail template key: {$key}");
        }

        $stored = self::storedEntry($key);
        $format = (string) ($stored['format'] ?? self::TEMPLATES[$key]['default_format']);
        $subjectTemplate = (string) ($stored['subject'] ?? self::defaultSubject($key));
        $bodyTemplate = self::defaultBody($key, $format);
        $vars = self::sampleVariables($key, $variables);

        return [
            'subject' => MailContentService::applyPlaceholders($subjectTemplate, $vars),
            'body' => MailContentService::applyPlaceholders($bodyTemplate, $vars),
            'format' => $format,
        ];
    }

    /**
     * @return array<string, array{subject: string, body: string}>
     */
    public static function presetsFor(string $key): array
    {
        return [
            MailContentService::FORMAT_MARKDOWN => [
                'subject' => self::defaultSubject($key),
                'body' => self::defaultBody($key, MailContentService::FORMAT_MARKDOWN),
            ],
            MailContentService::FORMAT_HTML => [
                'subject' => self::defaultSubject($key),
                'body' => self::defaultBody($key, MailContentService::FORMAT_HTML),
            ],
            MailContentService::FORMAT_PLAIN => [
                'subject' => self::defaultSubject($key),
                'body' => self::defaultBody($key, MailContentService::FORMAT_PLAIN),
            ],
        ];
    }

    public static function defaultSubject(string $key): string
    {
        return match ($key) {
            'smtp_test' => 'Uji Koneksi SMTP {{app_name}}',
            'forgot_password' => 'Reset Password — {{app_name}}',
            'welcome' => 'Selamat datang di {{app_name}}',
            'broadcast' => '{{title}} — {{app_name}}',
            'ticket_created' => 'Tiket #{{ticket_id}} dibuat — {{app_name}}',
            'ticket_updated' => 'Update tiket #{{ticket_id}} — {{app_name}}',
            'task_assigned' => 'Penugasan: {{pekerjaan_name}}',
            'data_reminder' => 'Lengkapi data Anda — {{app_name}}',
            'contract_ready' => 'Dokumen {{kontrak_name}} siap',
            'report_submitted' => 'Laporan {{report_name}} terkirim',
            default => 'Notifikasi {{app_name}}',
        };
    }

    public static function defaultBody(string $key, string $format = MailContentService::FORMAT_MARKDOWN): string
    {
        if ($key === 'smtp_test') {
            if ($format === MailContentService::FORMAT_HTML) {
                return self::defaultHtmlBody('smtp_test');
            }

            if ($format === MailContentService::FORMAT_PLAIN) {
                return self::defaultPlainBody('smtp_test');
            }

            return self::defaultMarkdownBody('smtp_test');
        }

        if ($format === MailContentService::FORMAT_HTML) {
            return self::defaultHtmlBody($key);
        }

        if ($format === MailContentService::FORMAT_PLAIN) {
            return self::defaultPlainBody($key);
        }

        return self::defaultMarkdownBody($key);
    }

    /**
     * @return array<string, string>
     */
    public static function sampleVariables(string $key, array $overrides = []): array
    {
        $appName = (string) ($overrides['app_name'] ?? AppSetting::getValue('app_name', 'Arumanis'));

        $defaults = [
            'app_name' => $appName,
            'user_name' => 'Budi Santoso',
            'user_email' => 'budi@example.com',
            'reset_link' => FrontendUrlService::to('/reset-password?token=contoh-token'),
            'expiry_minutes' => '60',
            'login_url' => FrontendUrlService::to('/login'),
            'title' => 'Pengumuman Penting',
            'message' => 'Ini contoh isi broadcast untuk semua pengguna.',
            'action_url' => FrontendUrlService::pengawasApp(),
            'ticket_id' => '1042',
            'ticket_title' => 'Permintaan akses modul kontrak',
            'ticket_url' => FrontendUrlService::to('/tiket/1042'),
            'status' => 'Diproses',
            'pekerjaan_name' => 'Pembangunan SPAM Desa Sukamaju',
            'pekerjaan_url' => FrontendUrlService::pengawasApp('pekerjaan/12'),
            'missing_fields' => 'Nomor telepon, foto profil',
            'profile_url' => FrontendUrlService::pengawasApp('profile'),
            'kontrak_name' => 'SPK-2026-001',
            'download_url' => FrontendUrlService::to('/kontrak/1/download'),
            'report_name' => 'Laporan Mingguan Maret 2026',
            'report_url' => FrontendUrlService::to('/laporan/88'),
        ];

        return array_merge($defaults, $overrides);
    }

    /**
     * @return array{format: string, subject: string, body?: string}|null
     */
    private static function storedEntry(string $key): ?array
    {
        $stored = self::storedMap()[$key] ?? null;

        if ($key === 'smtp_test' && $stored === null) {
            $stored = self::legacySmtpTestOverride();
        }

        if ($stored === null) {
            return null;
        }

        $format = (string) ($stored['format'] ?? self::TEMPLATES[$key]['default_format']);
        if (! in_array($format, [MailContentService::FORMAT_PLAIN, MailContentService::FORMAT_MARKDOWN, MailContentService::FORMAT_HTML], true)) {
            $format = self::TEMPLATES[$key]['default_format'];
        }

        $subject = trim((string) ($stored['subject'] ?? ''));
        $body = (string) ($stored['body'] ?? '');

        return [
            'format' => $format,
            'subject' => $subject !== '' ? $subject : self::defaultSubject($key),
            'body' => $body !== '' ? $body : null,
        ];
    }

    /**
     * @return array<string, array{format?: string, subject?: string, body?: string}>
     */
    private static function storedMap(): array
    {
        $raw = AppSetting::getValue(self::STORAGE_KEY, '');
        if ($raw === '') {
            return [];
        }

        $decoded = json_decode($raw, true);

        return is_array($decoded) ? $decoded : [];
    }

    /**
     * @return array{format: string, subject: string, body: string}|null
     */
    private static function legacySmtpTestOverride(): ?array
    {
        $legacyFormat = AppSetting::getValue('mail_body_format', '');
        $legacySubject = AppSetting::getValue('mail_subject', '');
        $legacyBody = AppSetting::getValue('mail_body', '');

        if ($legacyFormat === '' && $legacySubject === '' && $legacyBody === '') {
            return null;
        }

        $format = in_array($legacyFormat, [MailContentService::FORMAT_PLAIN, MailContentService::FORMAT_MARKDOWN, MailContentService::FORMAT_HTML], true)
            ? $legacyFormat
            : MailContentService::FORMAT_MARKDOWN;

        return [
            'format' => $format,
            'subject' => $legacySubject !== '' ? $legacySubject : self::defaultSubject('smtp_test'),
            'body' => $legacyBody !== '' ? $legacyBody : self::defaultBody('smtp_test', $format),
        ];
    }

    private static function storageUpdatedAt(): ?string
    {
        $setting = AppSetting::where('key', self::STORAGE_KEY)->first();

        return $setting?->updated_at?->toIso8601String();
    }

    private static function defaultMarkdownBody(string $key): string
    {
        return match ($key) {
            'smtp_test' => <<<'MD'
## Uji Koneksi SMTP

Email ini dikirim dari **Pengaturan Aplikasi** untuk memverifikasi konfigurasi SMTP **{{app_name}}**.

- Host, port, dan kredensial sudah benar jika email ini sampai ke Anda.
- Header logo dan footer ditambahkan otomatis saat dikirim.
MD,
            'forgot_password' => <<<'MD'
## Reset Password

Halo **{{user_name}}**,

Kami menerima permintaan reset password untuk akun **{{user_email}}**.

[Buat password baru]({{reset_link}})

Tautan berlaku selama **{{expiry_minutes}} menit**. Jika Anda tidak meminta reset, abaikan email ini.
MD,
            'welcome' => <<<'MD'
## Selamat datang, {{user_name}}!

Akun Anda di **{{app_name}}** telah aktif.

- Email: {{user_email}}
- [Masuk ke aplikasi]({{login_url}})

Jika Anda tidak merasa mendaftar, hubungi administrator.
MD,
            'broadcast' => <<<'MD'
## {{title}}

{{message}}

[Buka aplikasi pengawasan]({{action_url}})
MD,
            'ticket_created' => <<<'MD'
## Tiket #{{ticket_id}} dibuat

Halo **{{user_name}}**,

Tiket **{{ticket_title}}** telah dicatat di sistem.

[Buka tiket]({{ticket_url}})
MD,
            'ticket_updated' => <<<'MD'
## Update tiket #{{ticket_id}}

Halo **{{user_name}}**,

Status tiket **{{ticket_title}}** berubah menjadi **{{status}}**.

[Lihat tiket]({{ticket_url}})
MD,
            'task_assigned' => <<<'MD'
## Penugasan baru

Halo **{{user_name}}**,

Anda ditugaskan pada pekerjaan **{{pekerjaan_name}}**.

[Buka pekerjaan]({{pekerjaan_url}})
MD,
            'data_reminder' => <<<'MD'
## Lengkapi data Anda

Halo **{{user_name}}**,

Data berikut belum lengkap: **{{missing_fields}}**.

[Perbarui profil]({{profile_url}})
MD,
            'contract_ready' => <<<'MD'
## Dokumen siap

Halo **{{user_name}}**,

Dokumen **{{kontrak_name}}** sudah dapat diunduh.

[Unduh dokumen]({{download_url}})
MD,
            'report_submitted' => <<<'MD'
## Laporan terkirim

Halo **{{user_name}}**,

Laporan **{{report_name}}** berhasil dikirim.

[Lihat laporan]({{report_url}})
MD,
            default => 'Notifikasi dari **{{app_name}}**.',
        };
    }

    private static function defaultPlainBody(string $key): string
    {
        return match ($key) {
            'smtp_test' => "Uji Koneksi SMTP\n\nEmail ini memverifikasi konfigurasi SMTP {{app_name}}.\n\nJika Anda menerima pesan ini, pengaturan email sudah benar.",
            'forgot_password' => "Reset Password\n\nHalo {{user_name}},\n\nKami menerima permintaan reset password untuk {{user_email}}.\n\nBuat password baru: {{reset_link}}\n\nTautan berlaku {{expiry_minutes}} menit. Abaikan email ini jika Anda tidak meminta reset.",
            'welcome' => "Selamat datang, {{user_name}}!\n\nAkun Anda di {{app_name}} telah aktif.\n\nEmail: {{user_email}}\nMasuk: {{login_url}}",
            'broadcast' => "{{title}}\n\n{{message}}\n\nBuka aplikasi: {{action_url}}",
            'ticket_created' => "Tiket #{{ticket_id}} dibuat\n\nHalo {{user_name}},\n\nTiket {{ticket_title}} telah dicatat.\n\nBuka: {{ticket_url}}",
            'ticket_updated' => "Update tiket #{{ticket_id}}\n\nHalo {{user_name}},\n\nStatus {{ticket_title}}: {{status}}\n\nLihat: {{ticket_url}}",
            'task_assigned' => "Penugasan baru\n\nHalo {{user_name}},\n\nAnda ditugaskan pada {{pekerjaan_name}}.\n\nBuka: {{pekerjaan_url}}",
            'data_reminder' => "Lengkapi data Anda\n\nHalo {{user_name}},\n\nBelum lengkap: {{missing_fields}}\n\nPerbarui: {{profile_url}}",
            'contract_ready' => "Dokumen siap\n\nHalo {{user_name}},\n\n{{kontrak_name}} dapat diunduh.\n\nUnduh: {{download_url}}",
            'report_submitted' => "Laporan terkirim\n\nHalo {{user_name}},\n\n{{report_name}} berhasil dikirim.\n\nLihat: {{report_url}}",
            default => 'Notifikasi dari {{app_name}}.',
        };
    }

    private static function defaultHtmlBody(string $key): string
    {
        $L = MailLayoutService::class;

        return match ($key) {
            'smtp_test' => $L::heading('Uji Koneksi SMTP', 'Verifikasi konfigurasi email {{app_name}}')
                .$L::paragraph('Email ini dikirim dari Pengaturan Aplikasi untuk memastikan host, port, dan kredensial SMTP sudah benar.')
                .$L::infoBox(
                    $L::checkItem('Jika Anda membaca pesan ini, pengiriman email berfungsi.')
                    .$L::checkItem('Header memakai logo dari Pengaturan Aplikasi; warna disesuaikan otomatis.')
                )
                .$L::paragraph('Tidak perlu membalas email ini.'),

            'forgot_password' => $L::heading('Reset Password', 'Permintaan pemulihan akses akun Anda')
                .$L::greeting('{{user_name}}')
                .$L::paragraph('Kami menerima permintaan reset password untuk akun {{user_email}}. Klik tombol di bawah untuk membuat password baru.')
                .$L::button('Buat Password Baru', '{{reset_link}}')
                .$L::infoBox('Tautan berlaku <strong>{{expiry_minutes}} menit</strong>. Jika Anda tidak meminta reset, abaikan email ini.'),

            'welcome' => $L::heading('Selamat Datang!', 'Akun Anda di {{app_name}} telah aktif')
                .$L::greeting('{{user_name}}')
                .$L::paragraph('Terima kasih telah bergabung. Berikut ringkasan akun Anda:')
                .$L::bulletList([
                    'Email: <strong>{{user_email}}</strong>',
                ])
                .$L::button('Masuk ke Aplikasi', '{{login_url}}')
                .$L::paragraph('Jika Anda tidak merasa mendaftar, hubungi administrator.'),

            'broadcast' => $L::heading('{{title}}', 'Pengumuman dari {{app_name}}')
                .$L::messageBlock('{{message}}')
                .$L::button('Buka Aplikasi Pengawasan', '{{action_url}}'),

            'ticket_created' => $L::heading('Tiket Baru Dicatat', 'Nomor tiket #{{ticket_id}}')
                .$L::greeting('{{user_name}}')
                .$L::paragraph('Tiket bantuan Anda telah tercatat di sistem dan akan segera ditindaklanjuti.')
                .$L::infoBox('<strong>Judul:</strong> {{ticket_title}}<br><strong>Nomor:</strong> #{{ticket_id}}')
                .$L::button('Buka Tiket', '{{ticket_url}}'),

            'ticket_updated' => $L::heading('Update Tiket', 'Perubahan status tiket #{{ticket_id}}')
                .$L::greeting('{{user_name}}')
                .$L::paragraph('Ada pembaruan pada tiket bantuan Anda.')
                .$L::infoBox('<strong>Judul:</strong> {{ticket_title}}<br><strong>Status:</strong> {{status}}')
                .$L::button('Lihat Tiket', '{{ticket_url}}'),

            'task_assigned' => $L::heading('Penugasan Baru', 'Anda ditugaskan pada pekerjaan')
                .$L::greeting('{{user_name}}')
                .$L::paragraph('Anda ditugaskan untuk mengawasi pekerjaan berikut:')
                .$L::infoBox('<strong>{{pekerjaan_name}}</strong>')
                .$L::button('Buka Pekerjaan', '{{pekerjaan_url}}'),

            'data_reminder' => $L::heading('Lengkapi Data Anda', 'Profil atau data pekerjaan belum lengkap')
                .$L::greeting('{{user_name}}')
                .$L::paragraph('Mohon lengkapi data berikut agar proses pengawasan berjalan lancar:')
                .$L::infoBox('<strong>Belum lengkap:</strong> {{missing_fields}}')
                .$L::button('Perbarui Profil', '{{profile_url}}'),

            'contract_ready' => $L::heading('Dokumen Kontrak Siap', '{{kontrak_name}}')
                .$L::greeting('{{user_name}}')
                .$L::paragraph('Dokumen kontrak yang Anda butuhkan sudah tersedia dan dapat diunduh.')
                .$L::button('Unduh Dokumen', '{{download_url}}'),

            'report_submitted' => $L::heading('Laporan Terkirim', '{{report_name}}')
                .$L::greeting('{{user_name}}')
                .$L::paragraph('Laporan Anda telah berhasil dikirim dan tercatat di sistem.')
                .$L::button('Lihat Laporan', '{{report_url}}'),

            default => $L::paragraph('Notifikasi dari {{app_name}}.'),
        };
    }
}