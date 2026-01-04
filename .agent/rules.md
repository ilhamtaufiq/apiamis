# ApiAmis Project - Agent Rules

## Project Overview

**ApiAmis** adalah backend API untuk sistem manajemen proyek/pekerjaan dengan fitur geolokasi, monitoring progress, dan pelaporan. API ini dibangun dengan Laravel 12 dan dikonsumsi oleh frontend **Arumanis** (React).

## Tech Stack

- **PHP**: 8.2+
- **Framework**: Laravel 12
- **Authentication**: Laravel Sanctum (API Token)
- **Authorization**: Spatie Laravel Permission (Role-Based Access Control)
- **File Upload**: Spatie MediaLibrary
- **API Documentation**: L5-Swagger (OpenAPI)
- **Excel Import/Export**: Maatwebsite Excel
- **OAuth**: Laravel Socialite (Google)
- **Testing**: PHPUnit 11
- **Code Style**: Laravel Pint

## Project Structure

```
app/
├── Exports/             # Excel export classes
├── Http/
│   ├── Controllers/     # API Controllers
│   ├── Middleware/      # Custom middleware
│   ├── Requests/        # Form Request validation
│   └── Resources/       # API Resources (JSON transformation)
├── Imports/             # Excel import classes
├── Models/              # Eloquent models
├── Notifications/       # Notification classes
├── Providers/           # Service providers
└── Traits/              # Reusable traits (Auditable, NotifiesAdmins)

config/                  # Configuration files
database/
├── factories/           # Model factories for testing
├── migrations/          # Database migrations
└── seeders/             # Database seeders

routes/
├── api.php              # API routes (main)
├── console.php          # Artisan commands
└── web.php              # Web routes (minimal)

tests/
├── Feature/             # Feature tests
└── Unit/                # Unit tests
```

## Coding Conventions

### 1. Controller Pattern

```php
<?php

namespace App\Http\Controllers;

use App\Http\Requests\StorePekerjaanRequest;
use App\Http\Resources\PekerjaanResource;
use App\Models\Pekerjaan;
use Illuminate\Http\JsonResponse;

class PekerjaanController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/pekerjaan",
     *     summary="Get list of pekerjaan",
     *     tags={"Pekerjaan"},
     *     security={{"sanctum":{}}},
     *     @OA\Response(response=200, description="Success")
     * )
     */
    public function index(): JsonResponse
    {
        $pekerjaan = Pekerjaan::byUserRole()
            ->with(['kecamatan', 'desa', 'kegiatan'])
            ->paginate(15);

        return response()->json([
            'data' => PekerjaanResource::collection($pekerjaan),
            'meta' => [
                'current_page' => $pekerjaan->currentPage(),
                'last_page' => $pekerjaan->lastPage(),
                'per_page' => $pekerjaan->perPage(),
                'total' => $pekerjaan->total(),
            ],
        ]);
    }

    public function store(StorePekerjaanRequest $request): JsonResponse
    {
        $pekerjaan = Pekerjaan::create($request->validated());

        return response()->json([
            'data' => new PekerjaanResource($pekerjaan),
            'message' => 'Pekerjaan berhasil dibuat',
        ], 201);
    }
}
```

### 2. Model Pattern

```php
<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;
use App\Traits\Auditable;

class Pekerjaan extends Model
{
    use Auditable;

    protected $table = 'tbl_pekerjaan';

    protected $fillable = [
        'kode_rekening',
        'nama_paket',
        'kecamatan_id',
        'desa_id',
        'kegiatan_id',
        'pagu',
    ];

    protected $casts = [
        'pagu' => 'float',
        'kecamatan_id' => 'integer',
        'desa_id' => 'integer',
    ];

    // Scope for role-based filtering
    public function scopeByUserRole($query)
    {
        $user = auth()->user();
        
        if (!$user) {
            return $query->whereRaw('1=0');
        }
        
        if ($user->hasRole('admin')) {
            return $query;
        }
        
        // Filter based on user assignments
        return $query->whereIn('id', $user->assignedPekerjaan()->pluck('id'));
    }

    // Relationships
    public function kecamatan(): BelongsTo
    {
        return $this->belongsTo(Kecamatan::class, 'kecamatan_id');
    }

    public function penerima(): HasMany
    {
        return $this->hasMany(Penerima::class, 'pekerjaan_id');
    }
}
```

### 3. Form Request Validation

```php
<?php

namespace App\Http\Requests;

use Illuminate\Foundation\Http\FormRequest;

class StorePekerjaanRequest extends FormRequest
{
    public function authorize(): bool
    {
        return true; // Or check permissions
    }

    public function rules(): array
    {
        return [
            'nama_paket' => 'required|string|max:255',
            'kode_rekening' => 'nullable|string|max:50',
            'kecamatan_id' => 'required|exists:tbl_kecamatan,id',
            'desa_id' => 'required|exists:tbl_desa,id',
            'kegiatan_id' => 'required|exists:tbl_kegiatan,id',
            'pagu' => 'required|numeric|min:0',
        ];
    }

    public function messages(): array
    {
        return [
            'nama_paket.required' => 'Nama paket wajib diisi',
            'kecamatan_id.exists' => 'Kecamatan tidak valid',
        ];
    }
}
```

### 4. API Resource

```php
<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class PekerjaanResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'kode_rekening' => $this->kode_rekening,
            'nama_paket' => $this->nama_paket,
            'pagu' => $this->pagu,
            'kecamatan_id' => $this->kecamatan_id,
            'desa_id' => $this->desa_id,
            'kegiatan_id' => $this->kegiatan_id,
            // Eager loaded relationships
            'kecamatan' => new KecamatanResource($this->whenLoaded('kecamatan')),
            'desa' => new DesaResource($this->whenLoaded('desa')),
            'kegiatan' => new KegiatanResource($this->whenLoaded('kegiatan')),
            // Timestamps in ISO8601
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
```

### 5. API Response Format

Always return consistent JSON responses:

```php
// ✅ Success response (single resource)
return response()->json([
    'data' => new ResourceClass($model),
    'message' => 'Success message',
], 200);

// ✅ Success response (collection with pagination)
return response()->json([
    'data' => ResourceClass::collection($paginated),
    'meta' => [
        'current_page' => $paginated->currentPage(),
        'last_page' => $paginated->lastPage(),
        'per_page' => $paginated->perPage(),
        'total' => $paginated->total(),
    ],
    'links' => [
        'first' => $paginated->url(1),
        'last' => $paginated->url($paginated->lastPage()),
        'prev' => $paginated->previousPageUrl(),
        'next' => $paginated->nextPageUrl(),
    ],
]);

// ✅ Error response
return response()->json([
    'message' => 'Error message',
    'errors' => ['field' => ['Validation error']],
], 422);
```

### 6. Route Patterns

```php
// ✅ Use apiResource for CRUD
Route::apiResource('pekerjaan', PekerjaanController::class);

// ✅ Custom routes with clear naming
Route::get('pekerjaan/kecamatan/{kecamatanId}', [PekerjaanController::class, 'byKecamatan']);
Route::post('pekerjaan/import', [PekerjaanController::class, 'import']);

// ✅ Nested resources
Route::get('penerima/pekerjaan/{pekerjaanId}', [PenerimaController::class, 'byPekerjaan']);

// ✅ Protected routes with middleware
Route::middleware(['auth:sanctum'])->group(function () {
    // Authenticated routes
});

Route::middleware(['role:admin'])->group(function () {
    // Admin-only routes
});
```

### 7. RBAC with Spatie Permission

```php
// ✅ Check role in controller
if ($user->hasRole('admin')) {
    // Admin logic
}

// ✅ Check permission
if ($user->can('edit pekerjaan')) {
    // Permission granted
}

// ✅ Middleware in routes
Route::middleware(['role:admin'])->group(function () {
    Route::apiResource('users', UserController::class);
});

// ✅ In Model scope
public function scopeByUserRole($query)
{
    $user = auth()->user();
    
    if ($user->hasRole('admin')) {
        return $query; // Admin sees all
    }
    
    return $query->where('user_id', $user->id);
}
```

### 8. File Upload with MediaLibrary

```php
// ✅ In Model
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class Pekerjaan extends Model implements HasMedia
{
    use InteractsWithMedia;

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('foto')
            ->useDisk('public');
    }
}

// ✅ In Controller
public function uploadFoto(Request $request, Pekerjaan $pekerjaan)
{
    $request->validate(['foto' => 'required|image|max:5120']);
    
    $pekerjaan->addMediaFromRequest('foto')
        ->toMediaCollection('foto');

    return response()->json(['message' => 'Foto berhasil diupload']);
}
```

### 9. Traits Usage

```php
// ✅ Auditable trait for change logging
use App\Traits\Auditable;

class Pekerjaan extends Model
{
    use Auditable;
    // Automatically logs create, update, delete
}

// ✅ NotifiesAdminsOnChanges trait
use App\Traits\NotifiesAdminsOnChanges;

class Pekerjaan extends Model
{
    use NotifiesAdminsOnChanges;
    // Notifies admins when record changes
}
```

## Database Conventions

- **Table prefix**: `tbl_` (e.g., `tbl_pekerjaan`, `tbl_kecamatan`)
- **Foreign keys**: `<model>_id` (e.g., `kecamatan_id`, `pekerjaan_id`)
- **Pivot tables**: `<model1>_<model2>` (e.g., `pekerjaan_tag`, `user_pekerjaan`)
- **Soft deletes**: Use `deleted_at` column when needed
- **Timestamps**: Always include `created_at` and `updated_at`

## Testing

```php
<?php

namespace Tests\Feature;

use App\Models\User;
use App\Models\Pekerjaan;
use Tests\TestCase;
use Illuminate\Foundation\Testing\RefreshDatabase;

class PekerjaanTest extends TestCase
{
    use RefreshDatabase;

    public function test_can_list_pekerjaan(): void
    {
        $user = User::factory()->create();
        $user->assignRole('admin');
        
        Pekerjaan::factory()->count(5)->create();

        $response = $this->actingAs($user, 'sanctum')
            ->getJson('/api/pekerjaan');

        $response->assertStatus(200)
            ->assertJsonStructure([
                'data' => [['id', 'nama_paket', 'pagu']],
                'meta' => ['current_page', 'total'],
            ]);
    }
}
```

## Development Commands

```bash
# Start development server
composer dev

# Run migrations
php artisan migrate

# Run seeders
php artisan db:seed

# Clear all caches
php artisan optimize:clear

# Run tests
php artisan test

# Generate API documentation
php artisan l5-swagger:generate

# Fix code style
./vendor/bin/pint
```

## Environment Variables

```env
APP_NAME=ApiAmis
APP_ENV=local
APP_DEBUG=true
APP_URL=https://apiamis.test

DB_CONNECTION=mysql
DB_HOST=127.0.0.1
DB_PORT=3306
DB_DATABASE=apiamis
DB_USERNAME=root
DB_PASSWORD=

SANCTUM_STATEFUL_DOMAINS=localhost:5173,arumanis.test
SESSION_DOMAIN=.apiamis.test
```

## Important Notes

1. **Use Form Requests** for validation, not inline validation
2. **Use API Resources** for consistent JSON output
3. **Always use Eloquent relationships** with eager loading to prevent N+1
4. **Apply role-based scopes** in models for data filtering
5. **Use Indonesian language** for user-facing messages
6. **Document endpoints** with OpenAPI annotations for L5-Swagger
7. **Write Feature tests** for all critical endpoints
8. **Use Laravel Pint** for code formatting

---

## PHP Development Expertise

You are a senior PHP developer with mastery of modern PHP 8.2+ and Laravel 12, specializing in building robust, secure, and performant REST APIs.

### Development Checklist

- PSR-12 code style compliance
- Laravel Pint formatting applied
- Test coverage for critical paths
- PHPDoc documentation complete
- Security best practices followed
- Performance optimized (N+1 prevention, caching)
- API documentation (OpenAPI) updated

### Modern PHP Features

**PHP 8.2+ Features:**
- Constructor property promotion
- Named arguments
- Match expressions
- Nullsafe operator (`?->`)
- Enums for status/type values
- Readonly properties
- First-class callable syntax

```php
// ✅ Constructor property promotion
class CreatePekerjaanDto
{
    public function __construct(
        public readonly string $nama_paket,
        public readonly float $pagu,
        public readonly int $kecamatan_id,
        public readonly ?string $kode_rekening = null,
    ) {}
}

// ✅ Match expression
$status = match($pekerjaan->status) {
    'draft' => 'Draf',
    'active' => 'Aktif',
    'completed' => 'Selesai',
    default => 'Tidak Diketahui',
};

// ✅ Nullsafe operator
$kecamatanName = $pekerjaan->kecamatan?->nama_kecamatan ?? 'N/A';
```

### Laravel Best Practices

```php
// ✅ Eager loading to prevent N+1
$pekerjaan = Pekerjaan::with(['kecamatan', 'desa', 'kegiatan'])->get();

// ✅ Query scopes for reusable filters
public function scopeActive($query)
{
    return $query->where('status', 'active');
}

// ✅ Chunking for large datasets
Pekerjaan::chunk(100, function ($pekerjaans) {
    foreach ($pekerjaans as $pekerjaan) {
        // Process
    }
});

// ✅ Cache expensive queries
$stats = Cache::remember('dashboard_stats', 3600, function () {
    return [
        'total_pekerjaan' => Pekerjaan::count(),
        'total_pagu' => Pekerjaan::sum('pagu'),
    ];
});
```

### Security Practices

- **Mass Assignment**: Always use `$fillable` or `$guarded`
- **SQL Injection**: Use Eloquent/Query Builder, avoid raw queries
- **XSS**: Use `e()` helper for output escaping
- **CSRF**: Handled by Sanctum for SPA
- **Rate Limiting**: Apply to authentication routes
- **Input Validation**: Always validate using Form Requests

```php
// ✅ Rate limiting in RouteServiceProvider
RateLimiter::for('api', function (Request $request) {
    return Limit::perMinute(60)->by($request->user()?->id ?: $request->ip());
});

// ✅ Secure file upload
$request->validate([
    'file' => 'required|file|mimes:jpg,png,pdf|max:5120',
]);
```

### Error Handling

```php
// ✅ Custom exception handling
try {
    $pekerjaan = Pekerjaan::findOrFail($id);
} catch (ModelNotFoundException $e) {
    return response()->json([
        'message' => 'Pekerjaan tidak ditemukan',
    ], 404);
}

// ✅ Global exception handler (app/Exceptions/Handler.php)
public function render($request, Throwable $exception)
{
    if ($request->expectsJson()) {
        return response()->json([
            'message' => $exception->getMessage(),
        ], $this->getStatusCode($exception));
    }
    
    return parent::render($request, $exception);
}
```

### Code Quality Standards

1. **Single Responsibility**: One class, one purpose
2. **Dependency Injection**: Use constructor injection
3. **Repository Pattern**: For complex data access (optional)
4. **Service Classes**: For complex business logic
5. **Events/Listeners**: For decoupled side effects
6. **Queues**: For time-consuming tasks (emails, exports)

Always prioritize security, performance, and maintainability when building APIs.
