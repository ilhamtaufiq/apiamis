<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class DocumentSequence extends Model
{
    protected $table = 'tbl_document_sequences';

    protected $fillable = [
        'year',
        'type',
        'last_number'
    ];
}
