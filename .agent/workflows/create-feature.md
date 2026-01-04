---
description: Create a new feature module (Controller, Model, Migration, Resource)
---

# Create New Feature Module

When creating a new feature in ApiAmis, follow this workflow:

## 1. Create Migration

```bash
php artisan make:migration create_[feature]_table
```

Edit the migration file in `database/migrations/`:

```php
public function up(): void
{
    Schema::create('tbl_[feature]', function (Blueprint $table) {
        $table->id();
        $table->string('name');
        // Add other columns
        $table->timestamps();
    });
}
```

## 2. Create Model

```bash
php artisan make:model [Feature]
```

Edit `app/Models/[Feature].php`:

```php
<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class Feature extends Model
{
    protected $table = 'tbl_feature';

    protected $fillable = [
        'name',
        // Add fillable fields
    ];

    protected $casts = [
        // Type casts
    ];

    // Define relationships
    public function relatedModel(): BelongsTo
    {
        return $this->belongsTo(RelatedModel::class, 'related_id');
    }
}
```

## 3. Create API Resource

```bash
php artisan make:resource [Feature]Resource
```

Edit `app/Http/Resources/[Feature]Resource.php`:

```php
public function toArray(Request $request): array
{
    return [
        'id' => $this->id,
        'name' => $this->name,
        'created_at' => $this->created_at?->toIso8601String(),
        'updated_at' => $this->updated_at?->toIso8601String(),
    ];
}
```

## 4. Create Form Request (Validation)

```bash
php artisan make:request Store[Feature]Request
php artisan make:request Update[Feature]Request
```

Edit the request files in `app/Http/Requests/`:

```php
public function authorize(): bool
{
    return true;
}

public function rules(): array
{
    return [
        'name' => 'required|string|max:255',
    ];
}
```

## 5. Create Controller

```bash
php artisan make:controller [Feature]Controller --api
```

Edit `app/Http/Controllers/[Feature]Controller.php`:

```php
<?php

namespace App\Http\Controllers;

use App\Http\Requests\StoreFeatureRequest;
use App\Http\Resources\FeatureResource;
use App\Models\Feature;
use Illuminate\Http\JsonResponse;

class FeatureController extends Controller
{
    public function index(): JsonResponse
    {
        $features = Feature::paginate(15);

        return response()->json([
            'data' => FeatureResource::collection($features),
            'meta' => [
                'current_page' => $features->currentPage(),
                'last_page' => $features->lastPage(),
                'per_page' => $features->perPage(),
                'total' => $features->total(),
            ],
        ]);
    }

    public function store(StoreFeatureRequest $request): JsonResponse
    {
        $feature = Feature::create($request->validated());

        return response()->json([
            'data' => new FeatureResource($feature),
            'message' => 'Data berhasil dibuat',
        ], 201);
    }

    public function show(Feature $feature): JsonResponse
    {
        return response()->json([
            'data' => new FeatureResource($feature),
        ]);
    }

    public function update(UpdateFeatureRequest $request, Feature $feature): JsonResponse
    {
        $feature->update($request->validated());

        return response()->json([
            'data' => new FeatureResource($feature),
            'message' => 'Data berhasil diperbarui',
        ]);
    }

    public function destroy(Feature $feature): JsonResponse
    {
        $feature->delete();

        return response()->json([
            'message' => 'Data berhasil dihapus',
        ]);
    }
}
```

## 6. Add Routes

Edit `routes/api.php`:

```php
use App\Http\Controllers\FeatureController;

Route::middleware('auth:sanctum')->group(function () {
    Route::apiResource('features', FeatureController::class);
});
```

## 7. Run Migration

// turbo
```bash
php artisan migrate
```

## Checklist

- [ ] Migration created and run
- [ ] Model with fillable, casts, and relationships
- [ ] API Resource for JSON transformation
- [ ] Form Requests for validation
- [ ] Controller with CRUD methods
- [ ] Routes registered in api.php
- [ ] OpenAPI annotations added (optional)
- [ ] Feature test written (optional)
