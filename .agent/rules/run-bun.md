---
trigger: always_on
---

📋 Overview
ARUMANIS (Aplikasi Satu Data Air Minum dan Sanitasi) adalah sistem manajemen proyek infrastruktur untuk Air Minum dan Sanitasi. Ini adalah aplikasi frontend React yang terhubung dengan backend Laravel (apiamis).

🛠️ Tech Stack
Kategori	Teknologi
Framework	React 19 + TypeScript
Build Tool	Vite 7
Styling	Tailwind CSS 4
Routing	TanStack Router
State Management	Zustand
Form Handling	React Hook Form + Zod
UI Components	Radix UI + shadcn/ui
Authorization	CASL (role-based)
Data Fetching	TanStack Query
PDF Export	jsPDF + html2canvas
Charts	Recharts
📁 Struktur Project
src/
├── components/          # 53 reusable UI components
│   ├── layout/         # Sidebar, nav, header components (12 files)
│   └── ui/             # Base UI components (31 files)
├── config/             # App configuration (2 files)
├── context/            # React contexts (7 files)
├── features/           # 20 feature modules (90 files total)
├── hooks/              # Custom React hooks (4 files)
├── lib/                # Utility libraries (4 files)
├── routes/             # TanStack Router definitions (49 files)
└── stores/             # Zustand stores (2 files)
🔑 Feature Modules (20 total)
Module	Deskripsi
auth	Authentication (login/logout)
dashboard	Dashboard utama dengan statistik
kegiatan	Manajemen kegiatan/program
pekerjaan	Manajemen pekerjaan (17 files - terbesar)
kontrak	Manajemen kontrak
output	Output proyek
penerima	Data penerima manfaat
foto	Dokumentasi foto proyek
berkas	Manajemen dokumen
desa	Data desa
kecamatan	Data kecamatan
users	User management
roles	Role management
permissions	Permission management
route-permissions	Route-based permissions
menu-permissions	Menu-based permissions
kegiatan-role	Kegiatan-role mapping
settings	App settings
chat	AI Chat functionality
progress	Progress tracking
🚀 Scripts
Command	Fungsi
bun run dev	Development server (port 5173)
bun run build	Production build
bun run preview	Preview production build
bun run lint	ESLint checking
🔐 Key Features
✅ Authentication dengan Laravel Sanctum
✅ Role-Based Access Control menggunakan CASL
✅ Master Data Management (Kecamatan, Desa, Penyedia)
✅ Activity Management (Kegiatan)
✅ Job Tracking dengan kontrak, output, dan penerima
✅ Photo Documentation dengan koordinat GPS
✅ AI Chat untuk query data proyek
✅ PDF Export untuk laporan
✅ Dark Mode support
✅ Responsive Design
