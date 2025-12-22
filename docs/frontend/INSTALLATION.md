# 🚀 Panduan Instalasi - ARUMANIS

## 📋 Requirements

- **Bun**: ^1.0 (recommended) atau Node.js ^18.0
- **Git**
- Backend API (APIAMIS) running

---

## 📥 Instalasi

### 1. Clone Repository

```bash
git clone <repository-url> arumanis
cd arumanis
```

### 2. Install Dependencies

```bash
# Menggunakan Bun (recommended)
bun install

# Atau menggunakan npm
npm install
```

### 3. Environment Configuration

```bash
# Copy environment file
cp .env.example .env
```

### 4. Edit .env File

```env
# Development
VITE_API_URL=http://apiamis.test/api

# Production
# VITE_API_URL=https://apiamis.ilham.wtf/api
```

---

## 🖥️ Development Server

```bash
# Menggunakan Bun
bun run dev

# Menggunakan npm
npm run dev
```

Aplikasi akan berjalan di `http://localhost:5173`

---

## 🏗️ Build Production

```bash
# Build
bun run build

# Preview build
bun run preview
```

Output build akan ada di folder `dist/`

---

## 🧹 Linting

```bash
bun run lint
```

---

## ⚙️ Configuration

### Vite Config (vite.config.ts)

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    host: true,
  },
})
```

### TanStack Router Config (tsr.config.json)

```json
{
  "routesDirectory": "./src/routes",
  "generatedRouteTree": "./src/routeTree.gen.ts",
  "routeFileIgnorePrefix": "-"
}
```

### TypeScript Config (tsconfig.json)

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "jsx": "react-jsx",
    "baseUrl": ".",
    "paths": {
      "@/*": ["./src/*"]
    }
  }
}
```

### Tailwind Config

Tailwind CSS 4 menggunakan konfigurasi baru dengan CSS-based config.

```css
/* src/index.css */
@import "tailwindcss";

@theme {
  --color-primary: 221.2 83.2% 53.3%;
  --color-secondary: 210 40% 96.1%;
  --color-destructive: 0 84.2% 60.2%;
  /* ... */
}
```

---

## 🔌 API Connection

### Axios Instance (src/lib/api.ts)

```typescript
import axios from 'axios'

const api = axios.create({
  baseURL: import.meta.env.VITE_API_URL,
  withCredentials: true,
  headers: {
    'Content-Type': 'application/json',
    'Accept': 'application/json',
  }
})

// Interceptors
api.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      // Redirect to login
      window.location.href = '/login'
    }
    return Promise.reject(error)
  }
)

export default api
```

---

## 🐳 Docker Deployment

### Build Image

```bash
docker build -t arumanis-frontend .
```

### Run Container

```bash
docker run -d \
  --name arumanis \
  -p 80:80 \
  -e VITE_API_URL=https://apiamis.ilham.wtf/api \
  arumanis-frontend
```

### Dockerfile

```dockerfile
# Build stage
FROM oven/bun:1 as build
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install
COPY . .
RUN bun run build

# Production stage
FROM nginx:alpine
COPY --from=build /app/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/nginx.conf
EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
```

### Nginx Config (nginx.conf)

```nginx
events {
    worker_connections 1024;
}

http {
    include       /etc/nginx/mime.types;
    default_type  application/octet-stream;

    server {
        listen 80;
        server_name localhost;
        root /usr/share/nginx/html;
        index index.html;

        location / {
            try_files $uri $uri/ /index.html;
        }

        location /assets {
            expires 1y;
            add_header Cache-Control "public, immutable";
        }
    }
}
```

---

## 🔄 Auto-Redeploy (Coolify)

Project ini dikonfigurasi untuk auto-redeploy via **Coolify**. Setiap commit ke branch `main` akan trigger deployment via GitHub Webhook.

### Setup Steps:

1. Connect repository ke Coolify
2. Configure build command: `bun run build`
3. Configure output directory: `dist`
4. Set environment variables
5. Enable auto-deploy on push

---

## 🔧 Troubleshooting

### Error: Module not found

```bash
# Clear cache and reinstall
rm -rf node_modules bun.lock
bun install
```

### Error: CORS issues

1. Pastikan backend CORS dikonfigurasi dengan benar
2. Cek `VITE_API_URL` di `.env`
3. Pastikan `withCredentials: true` di axios config

### Error: Build failed

```bash
# Check TypeScript errors
bun run lint

# Clear Vite cache
rm -rf node_modules/.vite
bun run dev
```

### Error: Routes not working

```bash
# Regenerate route tree
bunx tsr generate
```

---

## 📱 Development Tips

### Hot Module Replacement (HMR)
Vite mendukung HMR secara default. Perubahan file akan langsung ter-update di browser.

### React DevTools
Install React DevTools extension untuk debugging yang lebih baik.

### TanStack Query DevTools
Query DevTools akan muncul di pojok kanan bawah saat development mode.

---

## 📚 Related Documentation

- [Architecture](./ARCHITECTURE.md)
- [Components](./COMPONENTS.md)
- [Features](./FEATURES.md)
