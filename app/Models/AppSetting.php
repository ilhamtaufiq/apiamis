<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;
use App\Traits\Auditable;

class AppSetting extends Model implements HasMedia
{
    use InteractsWithMedia, Auditable;

    protected $table = 'app_settings';

    protected $fillable = [
        'key',
        'value',
        'type',
    ];

    /**
     * PIN untuk membuka data sensitif penerima (NIK, Alamat) di PenerimaResource.
     * Default: '123456'.
     */
    public const PENERIMA_PIN_KEY = 'penerima_pin';

    /**
     * Judul berkas yang bisa dibuka ke role pengawas / konsultan_pengawas
     * lewat toggle pengaturan (1 = tampilkan).
     *
     * @var array<string, string>
     */
    public const PENGAWAS_BERKAS_JUDUL_KEYS = [
        'RAB' => 'pengawas_berkas_show_rab',
        'GAMBAR' => 'pengawas_berkas_show_gambar',
        'NEGO' => 'pengawas_berkas_show_nego',
    ];

    /**
     * Alias matching (case-insensitive) per kategori judul.
     * Dipakai backend filter + harus selaras dengan FE.
     *
     * @var array<string, list<string>>
     */
    public const PENGAWAS_BERKAS_JUDUL_ALIASES = [
        'RAB' => ['rab', 'r.a.b', 'r a b'],
        'GAMBAR' => ['gambar', 'gbr', 'g.b.r', 'g b r', 'drawing'],
        'NEGO' => ['nego', 'negosiasi', 'negos', 'hasil nego', 'hasil negosiasi'],
    ];

    /**
     * Get a setting value by key
     */
    public static function getValue(string $key, $default = null): ?string
    {
        $setting = static::where('key', $key)->first();
        return $setting ? $setting->value : $default;
    }

    /**
     * Judul berkas (RAB / GAMBAR / NEGO) yang diaktifkan untuk role lapangan.
     *
     * @return list<string>
     */
    public static function pengawasVisibleBerkasJuduls(): array
    {
        $titles = [];

        foreach (self::PENGAWAS_BERKAS_JUDUL_KEYS as $judul => $key) {
            if (self::getValue($key, '0') === '1') {
                $titles[] = $judul;
            }
        }

        return $titles;
    }

    /**
     * @return list<string>
     */
    public static function pengawasBerkasJudulAliases(string $judul): array
    {
        $key = strtoupper(trim($judul));

        return self::PENGAWAS_BERKAS_JUDUL_ALIASES[$key]
            ?? [mb_strtolower(trim($judul))];
    }

    /**
     * Compact alfanumerik lowercase: "G.B.R" / "gbr " → "gbr".
     */
    public static function compactBerkasJudul(string $value): string
    {
        return (string) preg_replace('/[^a-z0-9]+/u', '', mb_strtolower(trim($value)));
    }

    /**
     * Terapkan filter SQL: jenis_dokumen cocok ke salah satu judul aktif
     * (alias, case-insensitive, prefix, compact).
     *
     * @param  \Illuminate\Database\Eloquent\Builder<\App\Models\Berkas>|\Illuminate\Database\Query\Builder  $query
     * @param  list<string>  $titles  e.g. ['RAB','GAMBAR']
     */
    public static function applyPengawasSharedBerkasJudulFilter($query, array $titles): void
    {
        if ($titles === []) {
            return;
        }

        $query->where(function ($shared) use ($titles) {
            $first = true;

            foreach ($titles as $title) {
                foreach (self::pengawasBerkasJudulAliases((string) $title) as $alias) {
                    $alias = mb_strtolower(trim((string) $alias));
                    if ($alias === '') {
                        continue;
                    }

                    $compact = self::compactBerkasJudul($alias);
                    $apply = function ($q) use ($alias, $compact) {
                        $q->whereRaw('LOWER(TRIM(jenis_dokumen)) = ?', [$alias])
                            ->orWhereRaw('LOWER(TRIM(jenis_dokumen)) LIKE ?', [$alias.' %'])
                            ->orWhereRaw('LOWER(TRIM(jenis_dokumen)) LIKE ?', [$alias.'-%'])
                            ->orWhereRaw('LOWER(TRIM(jenis_dokumen)) LIKE ?', [$alias.'_%'])
                            ->orWhereRaw('LOWER(TRIM(jenis_dokumen)) LIKE ?', [$alias.'.%']);

                        if ($compact !== '') {
                            // "G.B.R", "GBR", "g b r" → gbr
                            $q->orWhereRaw(
                                "LOWER(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(TRIM(jenis_dokumen), ' ', ''), '.', ''), '-', ''), '_', ''), '/', '')) = ?",
                                [$compact]
                            )->orWhereRaw(
                                "LOWER(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(TRIM(jenis_dokumen), ' ', ''), '.', ''), '-', ''), '_', ''), '/', '')) LIKE ?",
                                [$compact.'%']
                            );
                        }
                    };

                    if ($first) {
                        $shared->where($apply);
                        $first = false;
                    } else {
                        $shared->orWhere($apply);
                    }
                }
            }

            // Jika semua alias kosong (tidak seharusnya), jangan buka semua baris.
            if ($first) {
                $shared->whereRaw('1 = 0');
            }
        });
    }

    /**
     * Set a setting value by key
     */
    public static function setValue(string $key, ?string $value, string $type = 'text'): self
    {
        return static::updateOrCreate(
            ['key' => $key],
            ['value' => $value, 'type' => $type]
        );
    }

    /**
     * Get all settings as key-value array
     */
    public static function getAllSettings(): array
    {
        $settings = static::all();
        $result = [];

        foreach ($settings as $setting) {
            if ($setting->type === 'file') {
                $media = $setting->getFirstMedia('app-settings');
                $result[$setting->key] = $media ? $media->getUrl() : null;
            } else {
                $result[$setting->key] = $setting->value;
            }
        }

        return $result;
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('app-settings')
            ->singleFile();
    }
}
