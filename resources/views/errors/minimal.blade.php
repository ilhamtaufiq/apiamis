<!DOCTYPE html>
<html lang="id" class="h-full bg-slate-950 text-slate-100">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>@yield('title') - Arumanis API</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Plus+Jakarta+Sans:wght@400;500;600;700;800&display=swap" rel="stylesheet">
    <style>
        body { font-family: 'Plus Jakarta Sans', sans-serif; }
    </style>
</head>
<body class="h-full flex items-center justify-center relative overflow-hidden bg-slate-950 selection:bg-indigo-500 selection:text-white">
    <!-- Ambient Gradient Background -->
    <div class="fixed inset-0 z-0 pointer-events-none opacity-40">
        <div class="absolute top-1/4 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-gradient-to-tr from-indigo-600/30 via-purple-600/20 to-pink-600/10 rounded-full blur-3xl animate-pulse"></div>
    </div>

    <!-- Main Container -->
    <div class="relative z-10 max-w-lg w-full px-6 text-center">
        <!-- Glassmorphic Status Hero -->
        <div class="mb-8 relative inline-flex flex-col items-center justify-center">
            <div class="absolute -inset-4 rounded-full bg-indigo-500/20 blur-2xl opacity-60"></div>
            <div class="relative flex flex-col items-center justify-center rounded-3xl border border-white/10 bg-slate-900/60 px-8 py-6 backdrop-blur-xl shadow-2xl shadow-black/50">
                <span class="text-6xl font-black tracking-tighter text-white drop-shadow-md">
                    @yield('code')
                </span>
                <span class="mt-1 text-xs font-bold uppercase tracking-[0.2em] text-indigo-400">
                    @yield('title')
                </span>
            </div>
        </div>

        <h1 class="text-2xl font-bold text-white mb-3 tracking-tight">
            @yield('title')
        </h1>

        <p class="text-slate-400 mb-8 text-sm leading-relaxed">
            @yield('message')
        </p>

        <div class="flex flex-col sm:flex-row items-center justify-center gap-3">
            <a href="/" class="w-full sm:w-auto">
                <button type="button" class="w-full sm:w-auto inline-flex items-center justify-center gap-2 px-6 py-2.5 rounded-xl bg-white text-slate-950 hover:bg-slate-200 font-bold text-sm transition-all shadow-lg shadow-white/10">
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 00-1-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6"/></svg>
                    Ke Beranda
                </button>
            </a>
            <button type="button" onclick="window.history.back()" class="w-full sm:w-auto inline-flex items-center justify-center gap-2 px-6 py-2.5 rounded-xl border border-white/10 bg-white/5 hover:bg-white/10 text-slate-300 font-semibold text-sm transition-all">
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 19l-7-7m0 0l7-7m-7 7h18"/></svg>
                Kembali
            </button>
        </div>

        <div class="mt-12 pt-6 border-t border-slate-900">
            <p class="text-[10px] uppercase tracking-[0.25em] font-bold text-slate-600">
                Arumanis HQ Ecosystem
            </p>
        </div>
    </div>
</body>
</html>
