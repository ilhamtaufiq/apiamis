<?php
try {
    $word = new COM("Word.Application") or die("Unable to instantiate Word");
    echo "Word installed version: " . $word->Version . "\n";
    $word->Quit();
} catch (Exception $e) {
    echo "Error: " . $e->getMessage() . "\n";
}
