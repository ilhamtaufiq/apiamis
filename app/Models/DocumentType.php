<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use App\Traits\Auditable;

class DocumentType extends Model
{
    use Auditable;
    protected $table = 'tbl_document_types';
    protected $fillable = ['name', 'code', 'format_template'];
}
