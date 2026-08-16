<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use App\Traits\Auditable;

class DocumentRegister extends Model
{
    use Auditable;
    protected $table = 'tbl_document_registers';
    protected $fillable = ['kontrak_id', 'type_id', 'addendum_id', 'attachment_type', 'nomor', 'tanggal', 'sequence_number', 'year', 'description', 'nilai'];

    protected $casts = [
        'tanggal' => 'date',
        'nilai' => 'float',
    ];

    public function kontrak()
    {
        return $this->belongsTo(Kontrak::class, 'kontrak_id');
    }

    public function type()
    {
        return $this->belongsTo(DocumentType::class, 'type_id');
    }

    public function addendum()
    {
        return $this->belongsTo(KontrakAddendum::class, 'addendum_id');
    }
}
