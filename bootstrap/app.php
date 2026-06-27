<?php

use Illuminate\Auth\AuthenticationException;
use Illuminate\Foundation\Application;
use Illuminate\Foundation\Configuration\Exceptions;
use Illuminate\Foundation\Configuration\Middleware;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Schema;
use Symfony\Component\HttpKernel\Exception\HttpExceptionInterface;

return Application::configure(basePath: dirname(__DIR__))
    ->withRouting(
        web: __DIR__.'/../routes/web.php',
        api: __DIR__.'/../routes/api.php',
        health: '/up',
    )
    ->withMiddleware(function (Middleware $middleware) {
        $middleware->trustProxies(at: '*');

        // 🔥 SPATIE LARAVEL PERMISSION - PATH BENAR
        $middleware->alias([
            'role' => \Spatie\Permission\Middleware\RoleMiddleware::class,
            'permission' => \Spatie\Permission\Middleware\PermissionMiddleware::class,
            'role_or_permission' => \Spatie\Permission\Middleware\RoleOrPermissionMiddleware::class,
            'check.route.permission' => \App\Http\Middleware\CheckRoutePermission::class,
        ]);

        // Sanctum
        $middleware->api(prepend: [
            \Laravel\Sanctum\Http\Middleware\EnsureFrontendRequestsAreStateful::class,
        ]);

        // Apply route permission check to all API routes (after auth)
        $middleware->api(append: [
            'check.route.permission',
        ]);

        $middleware->validateCsrfTokens(except: [
            'api/*',
        ]);
    })
    ->withExceptions(function (Exceptions $exceptions) {
        $exceptions->report(function (\Throwable $e) {
            if (! app()->bound('request')) {
                return;
            }

            $request = request();

            if ($e instanceof AuthenticationException) {
                return;
            }

            if ($e instanceof \Illuminate\Validation\ValidationException) {
                return;
            }

            if ($e instanceof \Illuminate\Auth\Access\AuthorizationException) {
                return;
            }

            if ($e instanceof HttpExceptionInterface && $e->getStatusCode() < 500) {
                return;
            }

            try {
                static $hasErrorLogsTable = null;
                $hasErrorLogsTable ??= Schema::hasTable('error_logs');

                if (! $hasErrorLogsTable) {
                    return;
                }

                \App\Models\ErrorLog::create([
                    'user_id' => Auth::id(),
                    'source' => 'backend',
                    'message' => $e->getMessage() ?: $e::class,
                    'stack' => $e->getTraceAsString(),
                    'url' => $request->fullUrl(),
                    'user_agent' => (string) $request->userAgent(),
                    'ip_address' => $request->ip(),
                    'metadata' => [
                        'exception' => $e::class,
                        'method' => $request->method(),
                        'route' => optional($request->route())->getName(),
                        'path' => $request->path(),
                    ],
                ]);
            } catch (\Throwable) {
                // Never let error reporting create a second exception cycle.
            }
        });

        // Return JSON 401 for unauthenticated API requests instead of redirecting to login
        $exceptions->render(function (AuthenticationException $e, Request $request) {
            if ($request->is('api/*') || $request->expectsJson()) {
                return response()->json([
                    'message' => 'Unauthenticated.',
                ], 401);
            }
        });

        // Handle RouteNotFoundException for API requests (Route [login] not defined)
        $exceptions->render(function (\Symfony\Component\Routing\Exception\RouteNotFoundException $e, Request $request) {
            if ($request->is('api/*') || $request->expectsJson()) {
                return response()->json([
                    'message' => 'Unauthenticated.',
                ], 401);
            }
        });
    })->create();
