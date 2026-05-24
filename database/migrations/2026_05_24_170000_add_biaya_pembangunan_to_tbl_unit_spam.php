<?php

use Illuminate\Database\Migrations\Migration;

return new class extends Migration
{
    /**
     * This migration is now a no-op.
     * The column was part of old budget fields that have been
     * moved to tbl_spam_budgets. Kept to avoid migration gaps.
     */
    public function up(): void
    {
        // no-op: biaya_pembangunan no longer needed in tbl_unit_spam
    }

    public function down(): void
    {
        // no-op
    }
};
