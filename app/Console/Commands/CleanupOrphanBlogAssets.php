<?php

namespace App\Console\Commands;

use App\Models\BlogAsset;
use Illuminate\Console\Command;

class CleanupOrphanBlogAssets extends Command
{
    protected $signature = 'blog-assets:cleanup-orphans {--hours=24 : Minimum asset age before deletion}';

    protected $description = 'Delete unattached blog assets older than the configured threshold';

    public function handle(): int
    {
        $hours = (int) $this->option('hours');
        $cutoff = now()->subHours($hours);
        $assets = BlogAsset::whereNull('blog_id')
            ->where('created_at', '<', $cutoff)
            ->get();

        foreach ($assets as $asset) {
            $asset->clearMediaCollection('blog/videos');
            $asset->delete();
        }

        $this->info("Deleted {$assets->count()} orphan blog assets.");

        return self::SUCCESS;
    }
}
