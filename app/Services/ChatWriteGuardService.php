<?php

namespace App\Services;

use App\Models\AuditLog;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Request;

class ChatWriteGuardService
{
    public const WRITE_TOOLS = ['create_output', 'create_penerima'];

    private const TTL_MINUTES = 10;

    public function __construct(
        private readonly ChatDataToolService $tools,
    ) {}

    public function isWriteTool(string $name): bool
    {
        return in_array($name, self::WRITE_TOOLS, true);
    }

    public function cacheKey(int $userId): string
    {
        return "pending_action_u{$userId}";
    }

    public function pending(int $userId): ?array
    {
        $draft = Cache::get($this->cacheKey($userId));

        return is_array($draft) && !empty($draft['tool']) && !empty($draft['args'])
            ? $draft
            : null;
    }

    public function forget(int $userId): void
    {
        Cache::forget($this->cacheKey($userId));
    }

    /**
     * Simpan rancangan tulis. Tidak mengeksekusi DB.
     *
     * @param  array<string, mixed>  $args
     * @return array<string, mixed>
     */
    public function stage(int $userId, ?int $sessionId, string $tool, array $args, ?string $summary = null): array
    {
        $payload = [
            'tool' => $tool,
            'args' => $args,
            'summary' => $summary ?? $this->summarize($tool, $args),
            'session_id' => $sessionId,
        ];
        Cache::put($this->cacheKey($userId), $payload, now()->addMinutes(self::TTL_MINUTES));

        return [
            'requires_confirmation' => true,
            'status' => 'requires_confirmation',
            'tool' => $tool,
            'args' => $args,
            'summary' => $payload['summary'],
            'hint' => 'Minta persetujuan user secara natural. Sebutkan detail data di summary. Jangan panggil ulang tool tulis sampai user mengonfirmasi (ya/setuju/simpan).',
        ];
    }

    public function isConfirmation(string $message): bool
    {
        $q = mb_strtolower(trim($message));
        if (preg_match('/^(ya|iya|yaudah|setuju|simpan|oke|ok|lanjutkan|lanjut|betul|benar|silakan|silahkan)\.?$/u', $q)) {
            return true;
        }

        return (bool) preg_match('/^(ya|iya|ok|oke|setuju|silakan|silahkan),?\s+(simpan|lanjut|saja|sudah|benar|setuju)\.?$/u', $q);
    }

    public function isCancellation(string $message): bool
    {
        $q = mb_strtolower(trim($message));

        return (bool) preg_match('/^(batal|batalkan|jangan|tidak|ga|gak|nggak|cancel)\.?$/u', $q);
    }

    /**
     * Validasi ulang draf, tulis dalam transaksi, catat AuditLog.
     *
     * @return array{ok: bool, tool: string, args?: array, result?: array, error?: string}|null
     */
    public function confirm(int $userId, ?int $sessionId): ?array
    {
        $draft = $this->pending($userId);
        if ($draft === null) {
            return null;
        }

        $tool = (string) $draft['tool'];
        $args = is_array($draft['args']) ? $draft['args'] : [];
        if (!$this->isWriteTool($tool)) {
            $this->forget($userId);

            return ['ok' => false, 'tool' => $tool, 'args' => $args, 'error' => 'Tool bukan aksi tulis yang diizinkan.'];
        }

        try {
            $result = DB::transaction(function () use ($tool, $args, $userId, $sessionId, $draft) {
                $out = $this->tools->execute($tool, $args);
                if (isset($out['error'])) {
                    throw new \RuntimeException((string) $out['error']);
                }
                $this->audit(
                    $userId,
                    $sessionId ?? ($draft['session_id'] ?? null),
                    $tool,
                    $args,
                    $out,
                );

                return $out;
            });
            $this->forget($userId);

            return ['ok' => true, 'tool' => $tool, 'args' => $args, 'result' => $result];
        } catch (\Throwable $e) {
            Log::warning('Chat write confirm failed', [
                'user_id' => $userId,
                'tool' => $tool,
                'error' => $e->getMessage(),
            ]);

            return ['ok' => false, 'tool' => $tool, 'args' => $args, 'error' => $e->getMessage()];
        }
    }

    public function summarize(string $tool, array $args): string
    {
        return match ($tool) {
            'create_output' => sprintf(
                'Tambah output %s (%s %s) ke paket ID %s',
                $args['komponen'] ?? '-',
                $args['volume'] ?? '-',
                $args['satuan'] ?? '-',
                $args['pekerjaan_id'] ?? '-',
            ),
            'create_penerima' => sprintf(
                'Tambah penerima %s (%s jiwa%s) ke paket ID %s',
                $args['nama'] ?? '-',
                $args['jumlah_jiwa'] ?? 1,
                !empty($args['is_komunal']) ? ', komunal' : '',
                $args['pekerjaan_id'] ?? '-',
            ),
            default => "Aksi {$tool}: " . json_encode($args, JSON_UNESCAPED_UNICODE),
        };
    }

    /**
     * @param  array<string, mixed>  $args
     * @param  array<string, mixed>  $result
     */
    private function audit(int $userId, mixed $sessionId, string $tool, array $args, array $result): void
    {
        try {
            AuditLog::create([
                'user_id' => $userId,
                'event' => 'chat_write_tool',
                'auditable_type' => \App\Models\ChatSession::class,
                'auditable_id' => (int) ($sessionId ?? 0),
                'old_values' => null,
                'new_values' => [
                    'chat_session_id' => $sessionId,
                    'tool' => $tool,
                    'args' => $args,
                    'result' => $result,
                ],
                'url' => Request::fullUrl(),
                'ip_address' => Request::ip(),
                'user_agent' => Request::userAgent(),
            ]);
        } catch (\Throwable $e) {
            Log::warning('Chat write audit failed', ['error' => $e->getMessage(), 'tool' => $tool]);
        }
    }
}
