<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class SpamKelembagaanSubmission extends Model
{
    public const STATUS_PENDING = 'pending';

    public const STATUS_APPROVED = 'approved';

    public const STATUS_REJECTED = 'rejected';

    protected $table = 'spam_kelembagaan_submissions';

    protected $fillable = [
        'share_link_id',
        'unit_spam_id',
        'payload',
        'snapshot_before',
        'submitter_name',
        'submitter_phone',
        'submitter_instansi',
        'submitter_note',
        'status',
        'reviewed_by',
        'reviewed_at',
        'review_note',
        'submitter_ip',
        'user_agent',
    ];

    protected $casts = [
        'payload' => 'array',
        'snapshot_before' => 'array',
        'reviewed_at' => 'datetime',
    ];

    public function shareLink(): BelongsTo
    {
        return $this->belongsTo(SpamKelembagaanShareLink::class, 'share_link_id');
    }

    public function unitSpam(): BelongsTo
    {
        return $this->belongsTo(UnitSpam::class, 'unit_spam_id');
    }

    public function reviewer(): BelongsTo
    {
        return $this->belongsTo(User::class, 'reviewed_by');
    }

    public function isPending(): bool
    {
        return $this->status === self::STATUS_PENDING;
    }
}
