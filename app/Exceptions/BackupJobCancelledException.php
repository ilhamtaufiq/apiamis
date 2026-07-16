<?php

namespace App\Exceptions;

use RuntimeException;

class BackupJobCancelledException extends RuntimeException
{
    public function __construct(string $message = 'Backup dibatalkan')
    {
        parent::__construct($message);
    }
}