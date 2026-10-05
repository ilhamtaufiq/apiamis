<?php

namespace App\Models;

use App\Traits\Auditable;
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
}
