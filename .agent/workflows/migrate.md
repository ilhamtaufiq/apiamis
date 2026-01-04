---
description: Run database migrations and seeders
---

# Database Migration Workflow

// turbo-all

## Run All Migrations

```bash
php artisan migrate
```

## Fresh Migration (Drop All Tables)

```bash
php artisan migrate:fresh
```

## Migration + Seeding

```bash
php artisan migrate:fresh --seed
```

## Rollback Last Migration

```bash
php artisan migrate:rollback
```

## Check Migration Status

```bash
php artisan migrate:status
```

## Create New Migration

```bash
php artisan make:migration create_[table_name]_table
php artisan make:migration add_[column]_to_[table]_table
```

## Run Specific Seeder

```bash
php artisan db:seed --class=UserSeeder
```

## Notes

- Always backup database before running `migrate:fresh` in production
- Use `--force` flag for production migrations
- Migrations are in `database/migrations/`
- Seeders are in `database/seeders/`
