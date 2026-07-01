<?php

namespace Tests\Unit;

use Tests\TestCase;

class ExcelDocumentExportServiceTest extends TestCase
{
    public function test_ringkasan_excel_template_exists(): void
    {
        $this->assertFileExists(storage_path('app/templates/ringkasan_kontrak_template.xlsx'));
    }

    public function test_ringkasan_template_definition_uses_xlsx(): void
    {
        $definition = \App\Services\KontrakTemplateService::TEMPLATES['kontrak_template_ringkasan'];

        $this->assertSame('ringkasan_kontrak_template.xlsx', $definition['default']);
        $this->assertSame('xlsx', $definition['format']);
    }
}