---
description: Start the development server with all services
---

# Start Dev Server

// turbo-all

The `composer dev` script starts multiple services concurrently:
- Laravel server
- Queue listener
- Pail (log viewer)
- Vite (assets)

## Start Development

```bash
composer dev
```

## Individual Commands

If you need to run services separately:

```bash
# Laravel server only
php artisan serve

# Queue listener
php artisan queue:listen --tries=1

# Log viewer
php artisan pail --timeout=0
```

## Notes

- API will be available at `https://apiamis.test` (via Laragon) or `http://127.0.0.1:8000`
- Make sure MySQL is running
- Check `.env` for database configuration
