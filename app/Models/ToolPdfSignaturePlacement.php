<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class ToolPdfSignaturePlacement extends Model
{
    protected $table = 'tool_pdf_signature_placements';

    protected $fillable = [
        'tool_pdf_id',
        'signature_id',
        'page_number',
        'x_ratio',
        'y_ratio',
        'scale',
        'sort_order',
        'signature_name',
        'signature_file_name',
        'signature_mime_type',
        'signature_width',
        'signature_height',
        'signature_data_url',
        'signature_source_type',
        'signature_source_id',
    ];

    protected $casts = [
        'page_number' => 'integer',
        'sort_order' => 'integer',
        'signature_width' => 'integer',
        'signature_height' => 'integer',
        'x_ratio' => 'float',
        'y_ratio' => 'float',
        'scale' => 'float',
    ];

    public function toolPdf(): BelongsTo
    {
        return $this->belongsTo(ToolPdf::class, 'tool_pdf_id');
    }
}
