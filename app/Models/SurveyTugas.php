<?php

namespace App\Models;

use App\Traits\Auditable;
use Illuminate\Database\Eloquent\Relations\BelongsToMany;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;

class SurveyTugas extends Model
{
    use HasFactory, Auditable;

    protected $table = 'tbl_survey_tugas';

    protected $fillable = [
        'pekerjaan_id',
        'judul',
        'tahun_anggaran',
        'jenis',
        'kecamatan_id',
        'desa_id',
        'lokasi_catatan',
        'assignee_id',
        'status',
        'batas_waktu',
        'catatan_admin',
        'created_by',
    ];

    protected $casts = [
        'tahun_anggaran' => 'integer',
        'batas_waktu' => 'date',
    ];

    protected $attributes = [
        'status' => 'ditugaskan',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    public function assignee(): BelongsTo
    {
        return $this->belongsTo(User::class, 'assignee_id');
    }

    public function creator(): BelongsTo
    {
        return $this->belongsTo(User::class, 'created_by');
    }

    public function kecamatan(): BelongsTo
    {
        return $this->belongsTo(Kecamatan::class, 'kecamatan_id');
    }

    public function desa(): BelongsTo
    {
        return $this->belongsTo(Desa::class, 'desa_id');
    }

    public function surveys(): HasMany
    {
        return $this->hasMany(SurveyLokasi::class, 'tugas_id');
    }

    /**
     * Semua penanggung jawab (assignee_id = utama, pertama di daftar).
     */
    public function assignees(): BelongsToMany
    {
        return $this->belongsToMany(User::class, 'tbl_survey_tugas_assignees', 'survey_tugas_id', 'user_id')
            ->withTimestamps();
    }

    public function isAssignee(int|string|null $userId): bool
    {
        if ($userId === null) {
            return false;
        }
        if ((string) $this->assignee_id === (string) $userId) {
            return true;
        }

        return $this->assignees()->where('users.id', $userId)->exists();
    }

    /**
     * @param list<int|string> $userIds
     */
    public function syncAssignees(array $userIds): void
    {
        $ids = array_values(array_unique(array_map('intval', array_filter($userIds))));
        if (empty($ids)) {
            return;
        }
        $this->assignees()->sync($ids);
        if ((string) $this->assignee_id !== (string) $ids[0]) {
            $this->forceFill(['assignee_id' => $ids[0]])->save();
        }
    }
}
