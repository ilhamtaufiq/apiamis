<?php

namespace App\Http\Resources;

use App\Models\Pekerjaan;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Illuminate\Support\Collection;

class AuditLogResource extends JsonResource
{
    /** @var Collection<int, string>|null */
    protected static ?Collection $pekerjaanNames = null;

    /**
     * @param  Collection<int, string>  $names
     */
    public static function withPekerjaanNames(Collection $names): void
    {
        self::$pekerjaanNames = $names;
    }

    public static function clearPekerjaanNames(): void
    {
        self::$pekerjaanNames = null;
    }

    /**
     * @return array<int, int>
     */
    public static function collectPekerjaanIds(iterable $logs): array
    {
        $ids = [];

        foreach ($logs as $log) {
            $id = self::resolvePekerjaanId($log);
            if ($id) {
                $ids[] = $id;
            }
        }

        return array_values(array_unique($ids));
    }

    public static function resolvePekerjaanId(object $log): ?int
    {
        $type = class_basename($log->auditable_type);
        $values = $log->new_values ?? $log->old_values ?? [];

        $pekerjaanId = match ($type) {
            'Pekerjaan' => (int) $log->auditable_id,
            'Kontrak' => isset($values['id_pekerjaan']) ? (int) $values['id_pekerjaan'] : null,
            'Output', 'Penerima', 'Foto', 'Berkas', 'Progress' => isset($values['pekerjaan_id']) ? (int) $values['pekerjaan_id'] : null,
            default => null,
        };

        return $pekerjaanId ?: null;
    }

    protected static function resolvePekerjaanTab(string $type): ?string
    {
        return match ($type) {
            'Kontrak' => 'kontrak',
            'Output' => 'output',
            'Penerima' => 'penerima',
            'Foto' => 'foto',
            'Berkas' => 'berkas',
            'Progress' => 'progress',
            default => null,
        };
    }

    /**
     * @return array<string, mixed>|null
     */
    protected function resolvePekerjaanContext(): ?array
    {
        $pekerjaanId = self::resolvePekerjaanId($this->resource);
        if (! $pekerjaanId) {
            return null;
        }

        $type = class_basename($this->auditable_type);
        $values = $this->new_values ?? $this->old_values ?? [];
        $namaPaket = $values['nama_paket'] ?? null;

        if (! $namaPaket && self::$pekerjaanNames) {
            $namaPaket = self::$pekerjaanNames->get($pekerjaanId);
        }

        $tab = self::resolvePekerjaanTab($type);

        return [
            'id' => $pekerjaanId,
            'nama_paket' => $namaPaket,
            'tab' => $tab,
        ];
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'user_id' => $this->user_id,
            'event' => $this->event,
            'auditable_type' => $this->auditable_type,
            'auditable_id' => $this->auditable_id,
            'old_values' => $this->old_values,
            'new_values' => $this->new_values,
            'url' => $this->url,
            'ip_address' => $this->ip_address,
            'user_agent' => $this->user_agent,
            'created_at' => $this->created_at,
            'updated_at' => $this->updated_at,
            'user' => $this->whenLoaded('user', fn () => [
                'id' => $this->user->id,
                'name' => $this->user->name,
                'email' => $this->user->email,
            ]),
            'pekerjaan' => $this->resolvePekerjaanContext(),
        ];
    }
}