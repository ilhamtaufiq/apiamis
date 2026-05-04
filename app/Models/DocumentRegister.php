<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class DocumentRegister extends Model
{
    protected $table = 'tbl_document_registers';
    protected $fillable = ['kontrak_id', 'type_id', 'nomor', 'tanggal', 'sequence_number', 'year', 'description'];

    public function kontrak()
    {
        return $this->belongsTo(Kontrak::class, 'kontrak_id');
    }

    public function type()
    {
        return $this->belongsTo(DocumentType::class, 'type_id');
    }
}
