---
trigger: always_on
---

# Backend Rules - APIAMIS

## 📋 Project Overview
APIAMIS adalah REST API backend untuk aplikasi ARUMANIS (Aplikasi Satu Data Air Minum dan Sanitasi). Backend ini menyediakan endpoint API untuk manajemen proyek infrastruktur Air Minum dan Sanitasi.

## 🛠️ Tech Stack
- **Framework**: Laravel 12
- **PHP**: ^8.2
- **Authentication**: Laravel Sanctum
- **Authorization**: Spatie Laravel Permission
- **Media**: Spatie Laravel MediaLibrary
- **API Documentation**: L5-Swagger
- **Database**: MySQL/PostgreSQL

## 📁 Project Structure
```
app/
├── Http/
│   ├── Controllers/     # API Controllers (22 files)
│   ├── Middleware/      # Custom middleware
│   └── Resources/       # API Resources for JSON responses (16 files)
├── Models/              # Eloquent Models (17 files)
└── Providers/           # Service Providers

database/
├── factories/           # Model factories for testing
├── migrations/          # Database migrations
└── seeders/             # Database seeders

routes/
├── api.php              # API routes
├── console.php          # Console commands
└── web.php              # Web routes

config/                  # Configuration files
tests/                   # PHPUnit tests
```

## 🎯 Coding Conventions

### Controllers
1. **File Naming**: Use PascalCase with `Controller` suffix (e.g., `PekerjaanController.php`)
2. **RESTful Actions**: Use Laravel's resource controller pattern (index, show, store, update, destroy)
3. **Validation**: Use Form Request classes for complex validation
4. **Response Format**: Use API Resources for consistent JSON responses

### Models
1. **Table Naming**: Use `tbl_` prefix for table names (e.g., `tbl_pekerjaan`)
2. **Relationships**: Define eloquent relationships in models
3. **Fillable/Guarded**: Always define `$fillable` or `$guarded` properties
4. **Casts**: Use `$casts` for attribute type casting

### API Resources
1. **File Naming**: Use PascalCase with `Resource` suffix (e.g., `PekerjaanResource.php`)
2. **Consistency**: Always return data through API Resources
3. **Nested Resources**: Include related data when needed

### Routing
1. **API Prefix**: All routes under `api/` prefix
2. **Resource Routes**: Use `apiResource()` for RESTful routes
3. **Authentication**: Protect routes with `auth:sanctum` middleware
4. **Role-based Access**: Use `role:admin` middleware for admin-only routes

### Authentication & Authorization
1. **Sanctum**: Use Laravel Sanctum for SPA authentication
2. **Spatie Permission**: Use for role-based access control
3. **Middleware**: Apply appropriate auth middleware to routes

### Database
1. **Migrations**: Write incremental migrations, never modify existing ones
2. **Seeders**: Create seeders for default data
3. **Factories**: Create factories for testing

## 🚀 Development Commands
```bash
# Development
php artisan serve           # Start development server
composer dev                # Run dev script (server + queue + logs + vite)

# Database
php artisan migrate         # Run migrations
php artisan db:seed         # Run seeders
php artisan migrate:fresh --seed  # Reset database with seeders

# Testing
php artisan test            # Run PHPUnit tests
composer test               # Run test script

# Code Quality
./vendor/bin/pint           # Laravel Pint code formatter

# Other
php artisan route:list      # List all routes
php artisan l5-swagger:generate  # Generate API documentation
```

## 📡 API Endpoints

### Authentication
- `POST /api/auth/login` - Login
- `POST /api/auth/logout` - Logout (protected)
- `GET /api/auth/me` - Get current user (protected)

### Master Data
- `GET/POST /api/kecamatan` - Kecamatan CRUD
- `GET/POST /api/desa` - Desa CRUD
- `GET/POST /api/penyedia` - Penyedia CRUD
- `GET/POST /api/kegiatan` - Kegiatan CRUD

### Core Resources
- `GET/POST /api/pekerjaan` - Pekerjaan CRUD
- `GET/POST /api/kontrak` - Kontrak CRUD
- `GET/POST /api/output` - Output CRUD
- `GET/POST /api/penerima` - Penerima CRUD
- `GET/POST /api/foto` - Foto CRUD
- `GET/POST /api/berkas` - Berkas CRUD

### User Management
- `GET/POST /api/users` - User CRUD
- `GET/POST /api/roles` - Role CRUD
- `GET/POST /api/permissions` - Permission CRUD

### Dashboard & Stats
- `GET /api/dashboard/stats` - Dashboard statistics

## ⚠️ Important Notes
1. **CORS**: Configured for frontend at `arumanis.test` and `arumanis.ilham.wtf`
2. **Media Storage**: Uses Spatie MediaLibrary for file uploads
3. **API Docs**: Available at `/api/documentation` via L5-Swagger
4. **Role Filtering**: Pekerjaan filtered by user's assigned kegiatan
5. **Soft Deletes**: Implement where appropriate

## 📝 Best Practices
1. Always validate input data using Form Requests
2. Use API Resources for consistent response format
3. Handle exceptions gracefully with proper HTTP status codes
4. Document API endpoints with Swagger annotations
5. Write tests for critical functionality
6. Use queues for heavy operations (email, file processing)
7. Cache frequently accessed data when appropriate
8. Follow Laravel naming conventions consistently
