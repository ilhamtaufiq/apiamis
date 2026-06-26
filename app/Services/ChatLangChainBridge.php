<?php

namespace App\Services;

use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Process;

class ChatLangChainBridge
{
    public function run(array $input): array
    {
        $result = Process::env(['PYTHONUNBUFFERED' => '1'])
            ->input(json_encode($input))
            ->timeout(120)
            ->run($this->command());

        if ($result->failed()) {
            Log::error('LangChain Script Error', [
                'error' => $result->errorOutput(),
                'output' => $result->output(),
            ]);

            return [
                'success' => false,
                'message' => 'LangChain execution failed: ' . ($result->errorOutput() ?: 'Unknown error'),
            ];
        }

        $output = json_decode($result->output(), true);
        if (!$output || !isset($output['success'])) {
            return [
                'success' => false,
                'message' => 'Invalid output format from LangChain script',
            ];
        }

        return $output;
    }

    /**
     * @param  callable(array<string, mixed>): void  $onEvent
     */
    public function stream(array $input, callable $onEvent): array
    {
        $input['stream'] = true;
        $process = Process::env(['PYTHONUNBUFFERED' => '1'])
            ->input(json_encode($input))
            ->timeout(180)
            ->start($this->command());

        $buffer = '';
        $final = [
            'success' => false,
            'message' => 'Streaming ended without completion event',
        ];

        while ($process->running()) {
            $buffer .= $process->latestOutput();
            $buffer = $this->consumeBuffer($buffer, $onEvent, $final);
            usleep(20_000);
        }

        $buffer .= $process->output();
        $this->consumeBuffer($buffer, $onEvent, $final, true);

        if ($process->failed() && empty($final['success'])) {
            Log::error('LangChain stream error', [
                'error' => $process->errorOutput(),
                'output' => $process->output(),
            ]);

            return [
                'success' => false,
                'message' => 'LangChain stream failed: ' . ($process->errorOutput() ?: 'Unknown error'),
            ];
        }

        return $final;
    }

    /**
     * @return array<int, string>
     */
    private function command(): array
    {
        $isWindows = PHP_OS_FAMILY === 'Windows';
        $pythonPath = $isWindows
            ? base_path('venv/Scripts/python.exe')
            : base_path('venv/bin/python');

        return [$pythonPath, '-u', base_path('scripts/chat_langchain.py')];
    }

    /**
     * @param  callable(array<string, mixed>): void  $onEvent
     * @param  array<string, mixed>  $final
     */
    private function consumeBuffer(string $buffer, callable $onEvent, array &$final, bool $flush = false): string
    {
        while (($pos = strpos($buffer, "\n")) !== false) {
            $line = trim(substr($buffer, 0, $pos));
            $buffer = substr($buffer, $pos + 1);

            if ($line === '') {
                continue;
            }

            $event = json_decode($line, true);
            if (!is_array($event) || !isset($event['type'])) {
                continue;
            }

            $onEvent($event);

            if (in_array($event['type'], ['done', 'tool_calls'], true)) {
                $final = $event;
            }

            if ($event['type'] === 'error') {
                $final = [
                    'success' => false,
                    'message' => $event['message'] ?? 'Streaming error',
                ];
            }
        }

        if ($flush && trim($buffer) !== '') {
            $event = json_decode(trim($buffer), true);
            if (is_array($event) && isset($event['type'])) {
                $onEvent($event);
                if (in_array($event['type'], ['done', 'tool_calls'], true)) {
                    $final = $event;
                }
            }
            return '';
        }

        return $buffer;
    }
}