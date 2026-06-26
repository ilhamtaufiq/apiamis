<?php

namespace App\Console\Commands;

use Illuminate\Console\Command;
use Illuminate\Support\Facades\Process;

class IndexChatKnowledge extends Command
{
    protected $signature = 'chat:index-knowledge';

    protected $description = 'Build BM25 knowledge index for chat RAG from docs/*.md';

    public function handle(): int
    {
        $isWindows = PHP_OS_FAMILY === 'Windows';
        $pythonPath = $isWindows
            ? base_path('venv/Scripts/python.exe')
            : base_path('venv/bin/python');
        $scriptPath = base_path('scripts/index_knowledge.py');

        if (!file_exists($pythonPath)) {
            $this->error('Python venv not found at ' . $pythonPath);
            return self::FAILURE;
        }

        if (!file_exists($scriptPath)) {
            $this->error('Indexer script not found at ' . $scriptPath);
            return self::FAILURE;
        }

        $result = Process::path(base_path('scripts'))
            ->run([$pythonPath, $scriptPath]);

        if ($result->failed()) {
            $this->error(trim($result->errorOutput() ?: $result->output() ?: 'Indexing failed'));
            return self::FAILURE;
        }

        $this->info(trim($result->output()));
        return self::SUCCESS;
    }
}