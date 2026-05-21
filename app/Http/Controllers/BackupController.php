<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\File;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Storage;
use Illuminate\Support\Str;
use Spatie\MediaLibrary\MediaCollections\Models\Media;
use ZipArchive;

class BackupController extends Controller
{
    private const BACKUP_DIR = 'system-backups';

    private const SQL_MARKER = "/*__ARUMANIS_STMT__*/\n";

    public function index()
    {
        $backupDir = $this->ensureBackupDirectory();
        $files = collect(Storage::disk('local')->files($backupDir))
            ->filter(fn ($file) => Str::endsWith($file, '.zip'))
            ->map(function (string $file) {
                $absolutePath = Storage::disk('local')->path($file);

                return [
                    'filename' => basename($file),
                    'size' => File::exists($absolutePath) ? File::size($absolutePath) : 0,
                    'last_modified' => File::exists($absolutePath) ? File::lastModified($absolutePath) : null,
                ];
            })
            ->sortByDesc('last_modified')
            ->values()
            ->map(function (array $item) {
                $item['download_url'] = url('/api/app-settings/backups/'.$item['filename']);

                return $item;
            });

        return response()->json([
            'data' => $files,
        ]);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'label' => 'nullable|string|max:80',
            'include_media' => 'nullable|boolean',
        ]);

        $includeMedia = $request->boolean('include_media', true);
        $jobId = (string) Str::uuid();
        $fileName = $this->buildBackupFilename($validated['label'] ?? null);

        $this->writeBackupJobStatus($jobId, [
            'job_id' => $jobId,
            'status' => 'queued',
            'filename' => $fileName,
            'include_media' => $includeMedia,
            'created_at' => now()->toIso8601String(),
            'message' => 'Backup masuk antrean',
        ]);

        app()->terminating(function () use ($jobId, $fileName, $includeMedia) {
            $this->runBackupJob($jobId, $fileName, $includeMedia);
        });

        return response()->json([
            'data' => $this->readBackupJobStatus($jobId),
            'message' => 'Backup sedang diproses di server',
        ], 202);
    }

    public function showJob(string $jobId)
    {
        $this->guardBackupJobId($jobId);
        $status = $this->readBackupJobStatus($jobId);

        abort_unless($status !== null, 404, 'Status backup tidak ditemukan');

        return response()->json([
            'data' => $status,
        ]);
    }

    public function download(string $filename)
    {
        $this->guardBackupFilename($filename);

        $path = Storage::disk('local')->path($this->backupFilePath($filename));
        abort_unless(File::exists($path), 404, 'Backup tidak ditemukan');

        return response()->download($path, $filename, [
            'Content-Type' => 'application/zip',
        ]);
    }

    public function destroy(string $filename)
    {
        $this->guardBackupFilename($filename);

        $path = $this->backupFilePath($filename);
        abort_unless(Storage::disk('local')->exists($path), 404, 'Backup tidak ditemukan');

        Storage::disk('local')->delete($path);

        return response()->json([
            'message' => 'Backup berhasil dihapus',
        ]);
    }

    public function restore(Request $request)
    {
        $validated = $request->validate([
            'backup_name' => 'nullable|string',
            'backup_file' => 'nullable|file|mimes:zip',
        ]);

        if (! $request->hasFile('backup_file') && empty($validated['backup_name'])) {
            return response()->json([
                'message' => 'Pilih file backup atau nama backup yang tersimpan',
            ], 422);
        }

        $sourcePath = null;
        $tempZipPath = null;

        try {
            if ($request->hasFile('backup_file')) {
                $tempZipPath = $request->file('backup_file')->storeAs(
                    'tmp',
                    'restore_'.Str::uuid().'.zip',
                    'local'
                );
                $sourcePath = Storage::disk('local')->path($tempZipPath);
            } else {
                $this->guardBackupFilename($validated['backup_name']);
                $sourcePath = Storage::disk('local')->path($this->backupFilePath($validated['backup_name']));
                abort_unless(File::exists($sourcePath), 404, 'Backup tidak ditemukan');
            }

            $result = $this->restoreArchive($sourcePath);

            return response()->json([
                'data' => $result,
                'message' => 'Restore backup berhasil dijalankan',
            ]);
        } finally {
            if ($tempZipPath) {
                Storage::disk('local')->delete($tempZipPath);
            }
        }
    }

    private function runBackupJob(string $jobId, string $fileName, bool $includeMedia): void
    {
        ignore_user_abort(true);
        @set_time_limit(0);

        $initialStatus = $this->readBackupJobStatus($jobId) ?? [];

        $this->writeBackupJobStatus($jobId, [
            'job_id' => $jobId,
            'status' => 'running',
            'filename' => $fileName,
            'include_media' => $includeMedia,
            'created_at' => $initialStatus['created_at'] ?? now()->toIso8601String(),
            'started_at' => now()->toIso8601String(),
            'message' => 'Backup sedang dibuat',
        ]);

        try {
            $result = $this->createBackupArchive($fileName, $includeMedia);
            $runningStatus = $this->readBackupJobStatus($jobId) ?? [];

            $this->writeBackupJobStatus($jobId, [
                'job_id' => $jobId,
                'status' => 'completed',
                'filename' => $fileName,
                'include_media' => $includeMedia,
                'created_at' => $runningStatus['created_at'] ?? now()->toIso8601String(),
                'started_at' => $runningStatus['started_at'] ?? null,
                'finished_at' => now()->toIso8601String(),
                'message' => 'Backup berhasil dibuat',
                'result' => $result,
            ]);
        } catch (\Throwable $exception) {
            Log::error('Backup job failed', [
                'job_id' => $jobId,
                'filename' => $fileName,
                'exception' => $exception,
            ]);

            $runningStatus = $this->readBackupJobStatus($jobId) ?? [];

            $this->writeBackupJobStatus($jobId, [
                'job_id' => $jobId,
                'status' => 'failed',
                'filename' => $fileName,
                'include_media' => $includeMedia,
                'created_at' => $runningStatus['created_at'] ?? now()->toIso8601String(),
                'started_at' => $runningStatus['started_at'] ?? null,
                'finished_at' => now()->toIso8601String(),
                'message' => 'Backup gagal dibuat',
                'error' => $exception->getMessage(),
            ]);
        }
    }

    private function createBackupArchive(string $fileName, bool $includeMedia): array
    {
        $backupDir = $this->ensureBackupDirectory();
        $zipPath = Storage::disk('local')->path($backupDir.DIRECTORY_SEPARATOR.$fileName);

        $sqlTempPath = tempnam(sys_get_temp_dir(), 'arumanis_sql_');
        if ($sqlTempPath === false) {
            throw new \RuntimeException('Gagal menyiapkan file backup');
        }

        try {
            $this->dumpDatabase($sqlTempPath);

            $zip = new ZipArchive;
            $result = $zip->open($zipPath, ZipArchive::CREATE | ZipArchive::OVERWRITE);
            if ($result !== true) {
                throw new \RuntimeException('Gagal membuka arsip backup');
            }

            $zip->addFile($sqlTempPath, 'database.sql');
            $zip->setCompressionName('database.sql', ZipArchive::CM_DEFLATE, 9);

            $mediaCount = 0;
            if ($includeMedia) {
                $mediaCount = $this->addMediaFilesToZip($zip);
            }

            $zip->setArchiveComment(json_encode([
                'created_at' => now()->toIso8601String(),
                'database' => DB::connection()->getDatabaseName(),
                'include_media' => $includeMedia,
                'media_files' => $mediaCount,
            ]));
            $zip->close();

            return [
                'filename' => $fileName,
                'download_url' => url('/api/app-settings/backups/'.$fileName),
                'size' => File::exists($zipPath) ? File::size($zipPath) : 0,
                'include_media' => $includeMedia,
                'media_files' => $mediaCount,
            ];
        } finally {
            @unlink($sqlTempPath);
        }
    }

    private function ensureBackupJobDirectory(): string
    {
        $dir = self::BACKUP_DIR.DIRECTORY_SEPARATOR.'jobs';
        if (! Storage::disk('local')->exists($dir)) {
            Storage::disk('local')->makeDirectory($dir);
        }

        return $dir;
    }

    private function backupJobFilePath(string $jobId): string
    {
        return $this->ensureBackupJobDirectory().DIRECTORY_SEPARATOR.$jobId.'.json';
    }

    private function guardBackupJobId(string $jobId): void
    {
        abort_unless(preg_match('/^[A-Za-z0-9-]+$/', $jobId) === 1, 422, 'ID backup tidak valid');
    }

    private function readBackupJobStatus(string $jobId): ?array
    {
        $this->guardBackupJobId($jobId);
        $path = $this->backupJobFilePath($jobId);
        if (! Storage::disk('local')->exists($path)) {
            return null;
        }

        $status = json_decode(Storage::disk('local')->get($path), true);

        return is_array($status) ? $status : null;
    }

    private function writeBackupJobStatus(string $jobId, array $status): void
    {
        $this->guardBackupJobId($jobId);
        Storage::disk('local')->put(
            $this->backupJobFilePath($jobId),
            json_encode($status, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES)
        );
    }

    private function ensureBackupDirectory(): string
    {
        $dir = self::BACKUP_DIR;
        if (! Storage::disk('local')->exists($dir)) {
            Storage::disk('local')->makeDirectory($dir);
        }

        return $dir;
    }

    private function backupFilePath(string $filename): string
    {
        return self::BACKUP_DIR.DIRECTORY_SEPARATOR.$filename;
    }

    private function buildBackupFilename(?string $label = null): string
    {
        $parts = ['arumanis', now()->format('Ymd_His')];

        if ($label !== null && trim($label) !== '') {
            $parts[] = Str::slug($label);
        }

        return implode('_', array_filter($parts)).'.zip';
    }

    private function guardBackupFilename(string $filename): void
    {
        abort_unless(preg_match('/^[A-Za-z0-9._-]+\.zip$/', $filename) === 1, 422, 'Nama backup tidak valid');
    }

    private function dumpDatabase(string $targetPath): void
    {
        $pdo = DB::connection()->getPdo();
        $schema = DB::connection()->getSchemaBuilder();
        $databaseName = DB::connection()->getDatabaseName();

        $tables = DB::select("SHOW FULL TABLES WHERE Table_type = 'BASE TABLE'");
        $tableNames = array_map(static function ($table) {
            return array_values((array) $table)[0];
        }, $tables);

        $handle = fopen($targetPath, 'wb');
        if ($handle === false) {
            throw new \RuntimeException('Gagal menulis file dump database');
        }

        try {
            fwrite($handle, "-- Arumanis backup\n");
            fwrite($handle, "-- Database: {$databaseName}\n");
            fwrite($handle, "SET FOREIGN_KEY_CHECKS=0;\n");
            fwrite($handle, "SET SQL_MODE='NO_AUTO_VALUE_ON_ZERO';\n");
            fwrite($handle, self::SQL_MARKER);

            foreach ($tableNames as $tableName) {
                $escapedTable = str_replace('`', '``', $tableName);
                $create = DB::selectOne("SHOW CREATE TABLE `{$escapedTable}`");
                $createSql = (string) ($create->{'Create Table'} ?? array_values((array) $create)[1] ?? '');

                fwrite($handle, "DROP TABLE IF EXISTS `{$escapedTable}`;\n".self::SQL_MARKER);
                fwrite($handle, $createSql.";\n".self::SQL_MARKER);

                $columns = $schema->getColumnListing($tableName);
                $rows = DB::table($tableName)->cursor();
                $batch = [];

                foreach ($rows as $row) {
                    $values = [];
                    foreach ($columns as $column) {
                        $values[] = $this->quoteValue($pdo, data_get($row, $column));
                    }

                    $batch[] = '('.implode(', ', $values).')';

                    if (count($batch) >= 100) {
                        fwrite($handle, $this->buildInsertStatement($escapedTable, $columns, $batch).self::SQL_MARKER);
                        $batch = [];
                    }
                }

                if ($batch !== []) {
                    fwrite($handle, $this->buildInsertStatement($escapedTable, $columns, $batch).self::SQL_MARKER);
                }
            }

            fwrite($handle, "SET FOREIGN_KEY_CHECKS=1;\n");
        } finally {
            fclose($handle);
        }
    }

    private function buildInsertStatement(string $table, array $columns, array $rows): string
    {
        $columnList = implode(', ', array_map(fn ($column) => '`'.str_replace('`', '``', $column).'`', $columns));

        return "INSERT INTO `{$table}` ({$columnList}) VALUES\n".implode(",\n", $rows).";\n";
    }

    private function quoteValue(\PDO $pdo, mixed $value): string
    {
        if ($value === null) {
            return 'NULL';
        }

        if (is_bool($value)) {
            return $value ? '1' : '0';
        }

        if (is_int($value) || is_float($value)) {
            return (string) $value;
        }

        if (is_array($value) || is_object($value)) {
            $value = json_encode($value, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
        }

        return $pdo->quote((string) $value);
    }

    private function addMediaFilesToZip(ZipArchive $zip): int
    {
        $mediaItems = Media::query()->get(['id', 'disk', 'file_name', 'collection_name', 'model_type', 'model_id']);
        $directories = [];

        foreach ($mediaItems as $media) {
            $disk = $media->disk ?: config('filesystems.default', 'public');
            $relativePath = $media->getPathRelativeToRoot();
            $directory = trim(str_replace(['\\', '/'], DIRECTORY_SEPARATOR, dirname($relativePath)), DIRECTORY_SEPARATOR);
            $directories[$disk][$directory === '.' ? '' : $directory] = true;
        }

        $count = 0;
        foreach ($directories as $disk => $paths) {
            foreach (array_keys($paths) as $path) {
                $files = Storage::disk($disk)->allFiles($path);
                foreach ($files as $file) {
                    $absolutePath = Storage::disk($disk)->path($file);
                    if (! File::exists($absolutePath)) {
                        continue;
                    }

                    $archiveName = 'media/'.$disk.'/'.ltrim(str_replace(['\\', '/'], '/', $file), '/');
                    $zip->addFile($absolutePath, $archiveName);
                    $zip->setCompressionName($archiveName, ZipArchive::CM_STORE);
                    $count++;
                }
            }
        }

        return $count;
    }

    private function restoreArchive(string $zipPath): array
    {
        $extractDir = storage_path('app/tmp/restore_'.Str::uuid());
        File::ensureDirectoryExists($extractDir);

        $zip = new ZipArchive;
        if ($zip->open($zipPath) !== true) {
            throw new \RuntimeException('Gagal membuka file backup');
        }

        try {
            $zip->extractTo($extractDir);
            $zip->close();

            $sqlPath = $extractDir.DIRECTORY_SEPARATOR.'database.sql';
            abort_unless(File::exists($sqlPath), 422, 'Database dump tidak ditemukan di backup');

            DB::statement('SET FOREIGN_KEY_CHECKS=0');
            $sql = File::get($sqlPath);
            $statements = array_values(array_filter(array_map('trim', explode(self::SQL_MARKER, $sql))));

            foreach ($statements as $statement) {
                if ($statement === '' || Str::startsWith($statement, '--')) {
                    continue;
                }

                DB::unprepared(rtrim($statement, ";\r\n\t "));
            }

            $this->restoreMediaFiles($extractDir);
            DB::statement('SET FOREIGN_KEY_CHECKS=1');

            return [
                'restored_at' => now()->toIso8601String(),
                'source' => basename($zipPath),
            ];
        } finally {
            DB::statement('SET FOREIGN_KEY_CHECKS=1');
            File::deleteDirectory($extractDir);
        }
    }

    private function restoreMediaFiles(string $extractDir): void
    {
        $mediaRoot = $extractDir.DIRECTORY_SEPARATOR.'media';
        if (! File::exists($mediaRoot)) {
            return;
        }

        $files = File::allFiles($mediaRoot);
        foreach ($files as $file) {
            $relative = Str::after($file->getPathname(), $mediaRoot.DIRECTORY_SEPARATOR);
            $relative = str_replace(['\\', '/'], DIRECTORY_SEPARATOR, $relative);
            $parts = explode(DIRECTORY_SEPARATOR, $relative, 2);

            if (count($parts) < 2) {
                continue;
            }

            [$disk, $path] = $parts;
            $targetPath = Storage::disk($disk)->path($path);
            File::ensureDirectoryExists(dirname($targetPath));
            File::copy($file->getPathname(), $targetPath);
        }
    }
}
