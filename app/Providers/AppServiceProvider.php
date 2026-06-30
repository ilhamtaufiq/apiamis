<?php

namespace App\Providers;

use App\Services\MailConfigService;
use Illuminate\Cache\RateLimiting\Limit;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\RateLimiter;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\URL;
use Illuminate\Support\ServiceProvider;

class AppServiceProvider extends ServiceProvider
{
    /**
     * Register any application services.
     */
    public function register(): void
    {
        //
    }

    /**
     * Bootstrap any application services.
     */
    public function boot(): void
    {
        $appUrl = rtrim((string) config('app.url'), '/');

        if ($appUrl !== '') {
            URL::forceRootUrl($appUrl);
        }

        if (str_starts_with($appUrl, 'https://') || $this->app->environment('production')) {
            URL::forceScheme('https');
        }

        RateLimiter::for('login', function (Request $request) {
            $email = (string) $request->input('email');

            return [
                Limit::perMinute(5)->by($request->ip()),
                Limit::perMinute(5)->by($email !== '' ? strtolower($email) : $request->ip()),
            ];
        });

        RateLimiter::for('handoff-exchange', function (Request $request) {
            return Limit::perMinute(20)->by($request->ip());
        });

        RateLimiter::for('blog-comments', function (Request $request) {
            $userId = $request->user()?->id;

            return Limit::perMinute(10)->by($userId ? 'user:' . $userId : $request->ip());
        });

        RateLimiter::for('contact-inquiries', function (Request $request) {
            $email = strtolower(trim((string) $request->input('email')));

            return [
                Limit::perMinute(3)->by($request->ip()),
                Limit::perHour(10)->by($request->ip()),
                Limit::perHour(5)->by($email !== '' ? 'email:'.$email : $request->ip()),
            ];
        });

        try {
            if (Schema::hasTable('app_settings')) {
                MailConfigService::applyFromSettings();
            }
        } catch (\Throwable) {
            // Ignore during install/migrate when DB is unavailable.
        }
    }
}
