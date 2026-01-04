---
description: Generate or update OpenAPI documentation
---

# Generate API Documentation

// turbo-all

## Generate Swagger/OpenAPI Docs

```bash
php artisan l5-swagger:generate
```

## View Documentation

After generating, access the documentation at:
- `https://apiamis.test/api/documentation`

## Adding OpenAPI Annotations

Add annotations to your controller methods:

```php
/**
 * @OA\Get(
 *     path="/api/pekerjaan",
 *     summary="Get list of pekerjaan",
 *     description="Returns paginated list of pekerjaan based on user role",
 *     tags={"Pekerjaan"},
 *     security={{"sanctum":{}}},
 *     @OA\Parameter(
 *         name="page",
 *         in="query",
 *         description="Page number",
 *         required=false,
 *         @OA\Schema(type="integer", default=1)
 *     ),
 *     @OA\Parameter(
 *         name="search",
 *         in="query",
 *         description="Search term",
 *         required=false,
 *         @OA\Schema(type="string")
 *     ),
 *     @OA\Response(
 *         response=200,
 *         description="Success",
 *         @OA\JsonContent(
 *             @OA\Property(property="data", type="array", @OA\Items(ref="#/components/schemas/Pekerjaan")),
 *             @OA\Property(property="meta", type="object")
 *         )
 *     ),
 *     @OA\Response(response=401, description="Unauthenticated")
 * )
 */
public function index(): JsonResponse
{
    // ...
}
```

## Schema Definition

Add schema annotations in your controller or a dedicated file:

```php
/**
 * @OA\Schema(
 *     schema="Pekerjaan",
 *     @OA\Property(property="id", type="integer", example=1),
 *     @OA\Property(property="nama_paket", type="string", example="Pembangunan Saluran Air"),
 *     @OA\Property(property="pagu", type="number", format="float", example=100000000),
 *     @OA\Property(property="created_at", type="string", format="date-time")
 * )
 */
```

## Configuration

Swagger configuration is in `config/l5-swagger.php`.

## Notes

- Run `php artisan l5-swagger:generate` after adding/modifying annotations
- Documentation is auto-cached; clear cache if not updating: `php artisan config:clear`
