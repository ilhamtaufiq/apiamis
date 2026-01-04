---
description: Run tests for the ApiAmis project
---

# Testing Workflow

// turbo-all

## Run All Tests

```bash
php artisan test
```

Or using composer script:

```bash
composer test
```

## Run Specific Test File

```bash
php artisan test tests/Feature/PekerjaanTest.php
```

## Run Specific Test Method

```bash
php artisan test --filter=test_can_list_pekerjaan
```

## Run with Coverage

```bash
php artisan test --coverage
```

## Run in Parallel

```bash
php artisan test --parallel
```

## Writing Tests

Tests should be placed in:
- `tests/Feature/` - for API endpoint tests
- `tests/Unit/` - for unit tests

### Example Feature Test

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

    public function test_can_create_pekerjaan(): void
    {
        $user = User::factory()->create();
        $user->assignRole('admin');

        $payload = [
            'nama_paket' => 'Test Pekerjaan',
            'pagu' => 100000000,
            'kecamatan_id' => 1,
            'desa_id' => 1,
            'kegiatan_id' => 1,
        ];

        $response = $this->actingAs($user, 'sanctum')
            ->postJson('/api/pekerjaan', $payload);

        $response->assertStatus(201)
            ->assertJson(['message' => 'Pekerjaan berhasil dibuat']);
    }
}
```

## Notes

- Use `RefreshDatabase` trait for clean database state
- Use `actingAs($user, 'sanctum')` for authenticated requests
- Create test data with factories
