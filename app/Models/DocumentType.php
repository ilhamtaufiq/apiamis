<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class DocumentType extends Model
{
    protected $table = 'tbl_document_types';
    protected $fillable = ['name', 'code', 'format_template'];
}
