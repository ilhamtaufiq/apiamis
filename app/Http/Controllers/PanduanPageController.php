<?php

namespace App\Http\Controllers;

use App\Http\Resources\PanduanPageResource;
use App\Models\PanduanPage;
use Illuminate\Http\Request;
use Illuminate\Support\Str;
use Illuminate\Validation\Rule;

class PanduanPageController extends Controller
{
    /** Public: published pages only (for /docs). */
    public function publicIndex(Request $request)
    {
        $query = PanduanPage::query()->published()->ordered();

        if ($section = $request->query('section')) {
            $query->where('section', $section);
        }

        // List without full body for nav trees
        if ($request->boolean('summary')) {
            $pages = $query->get(['id', 'slug', 'title', 'description', 'section', 'sort_order', 'is_published', 'updated_at']);

            return response()->json(['data' => $pages]);
        }

        return PanduanPageResource::collection($query->get());
    }

    /** Public: single published page by slug. */
    public function publicShow(string $slug)
    {
        $page = PanduanPage::query()->published()->where('slug', $slug)->firstOrFail();

        return new PanduanPageResource($page);
    }

    /** Admin list (includes drafts). */
    public function index(Request $request)
    {
        $query = PanduanPage::query()->with('editor:id,name')->ordered();

        if ($request->filled('section')) {
            $query->where('section', $request->query('section'));
        }

        if ($request->has('published')) {
            $query->where('is_published', $request->boolean('published'));
        }

        if ($search = trim((string) $request->query('search', ''))) {
            $query->where(function ($q) use ($search) {
                $q->where('title', 'like', "%{$search}%")
                    ->orWhere('slug', 'like', "%{$search}%");
            });
        }

        return PanduanPageResource::collection($query->get());
    }

    public function store(Request $request)
    {
        $validated = $this->validated($request);
        $validated['updated_by'] = $request->user()?->id;
        $validated['slug'] = $this->normalizeSlug($validated['slug'] ?? $validated['title']);

        $page = PanduanPage::create($validated);

        return (new PanduanPageResource($page->load('editor:id,name')))
            ->response()
            ->setStatusCode(201);
    }

    public function show(PanduanPage $panduan)
    {
        $panduan->load('editor:id,name');

        return new PanduanPageResource($panduan);
    }

    public function update(Request $request, PanduanPage $panduan)
    {
        $validated = $this->validated($request, $panduan->id);
        $validated['updated_by'] = $request->user()?->id;
        if (isset($validated['slug'])) {
            $validated['slug'] = $this->normalizeSlug($validated['slug']);
        }

        $panduan->update($validated);

        return new PanduanPageResource($panduan->fresh()->load('editor:id,name'));
    }

    public function destroy(PanduanPage $panduan)
    {
        $panduan->delete();

        return response()->json(['message' => 'Halaman panduan dihapus']);
    }

    /**
     * Seed default pages if table empty (from built-in stubs).
     */
    public function seedDefaults(Request $request)
    {
        $force = $request->boolean('force');
        $created = 0;
        $skipped = 0;

        foreach ($this->defaultPages() as $stub) {
            $exists = PanduanPage::where('slug', $stub['slug'])->exists();
            if ($exists && ! $force) {
                $skipped++;

                continue;
            }

            PanduanPage::updateOrCreate(
                ['slug' => $stub['slug']],
                array_merge($stub, [
                    'updated_by' => $request->user()?->id,
                ])
            );
            $created++;
        }

        return response()->json([
            'message' => "Seed selesai: {$created} disimpan, {$skipped} dilewati",
            'created' => $created,
            'skipped' => $skipped,
        ]);
    }

    private function validated(Request $request, ?int $ignoreId = null): array
    {
        return $request->validate([
            'slug' => [
                'nullable',
                'string',
                'max:120',
                'regex:/^[a-z0-9]+(?:-[a-z0-9]+)*$/',
                Rule::unique('panduan_pages', 'slug')->ignore($ignoreId),
            ],
            'title' => 'required|string|max:255',
            'description' => 'nullable|string|max:500',
            'section' => 'nullable|string|max:80',
            'sort_order' => 'nullable|integer|min:0|max:9999',
            'body' => 'required|string',
            'is_published' => 'sometimes|boolean',
        ]);
    }

    private function normalizeSlug(string $value): string
    {
        $slug = Str::slug($value);

        return $slug !== '' ? $slug : 'halaman-'.Str::random(6);
    }

    /**
     * @return list<array{slug: string, title: string, description: string, section: string, sort_order: int, body: string, is_published: bool}>
     */
    private function defaultPages(): array
    {
        return [
            [
                'slug' => 'beranda-cms',
                'title' => 'Beranda (konten dinamis)',
                'description' => 'Contoh halaman dikelola dari dashboard. Bisa diedit admin kapan saja.',
                'section' => 'cms',
                'sort_order' => 0,
                'is_published' => true,
                'body' => <<<'MD'
# Beranda dinamis

Halaman ini disimpan di database dan bisa diubah lewat **Manajemen Panduan** di dashboard Arumanis.

## Cara pakai

1. Buka menu **Pengaturan → Manajemen Panduan**
2. Edit judul, deskripsi, dan isi Markdown
3. Centang **Terbit** agar tampil di `/docs`
4. Simpan — perubahan langsung aktif tanpa rebuild Docker

## Format

Gunakan Markdown: judul `#`, daftar, tabel, dan tautan `[teks](/docs/slug-lain)`.
MD,
            ],
            [
                'slug' => 'catatan-rilis',
                'title' => 'Catatan rilis panduan',
                'description' => 'Changelog singkat untuk operator (diedit admin).',
                'section' => 'cms',
                'sort_order' => 10,
                'is_published' => true,
                'body' => <<<'MD'
# Catatan rilis

- Manajemen panduan CMS diaktifkan
- Admin dapat menambah/mengedit halaman Markdown
- Halaman terbit tampil di situs dokumentasi `/docs`

Edit halaman ini untuk mengumumkan perubahan prosedur internal.
MD,
            ],
        ];
    }
}
