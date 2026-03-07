<?php
    $lines = file(__DIR__ . "/storage/logs/laravel.log", FILE_IGNORE_NEW_LINES);
    $last_error = "";
    foreach ($lines as $line) {
        if (strpos($line, 'local.ERROR') !== false) {
            $last_error = $line;
        }
    }
    echo "ERROR: " . $last_error . "\n";
