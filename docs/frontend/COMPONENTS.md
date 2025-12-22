# 🧩 UI Components - ARUMANIS

## 📋 Overview

ARUMANIS menggunakan komponen UI dari shadcn/ui yang dibangun di atas Radix UI primitives dan Tailwind CSS.

---

## 📁 Struktur Components

```
src/components/
├── layout/              # Layout components
│   ├── Sidebar.tsx
│   ├── Header.tsx
│   ├── MainLayout.tsx
│   └── ...
│
└── ui/                  # Base UI components (shadcn/ui)
    ├── accordion.tsx
    ├── alert.tsx
    ├── alert-dialog.tsx
    ├── badge.tsx
    ├── button.tsx
    ├── card.tsx
    ├── checkbox.tsx
    ├── dialog.tsx
    ├── dropdown-menu.tsx
    ├── form.tsx
    ├── input.tsx
    ├── label.tsx
    ├── popover.tsx
    ├── select.tsx
    ├── separator.tsx
    ├── sheet.tsx
    ├── skeleton.tsx
    ├── table.tsx
    ├── tabs.tsx
    ├── textarea.tsx
    ├── toast.tsx
    ├── tooltip.tsx
    └── ...
```

---

## 🎨 Base Components

### Button

```tsx
import { Button } from "@/components/ui/button"

// Variants
<Button variant="default">Default</Button>
<Button variant="destructive">Destructive</Button>
<Button variant="outline">Outline</Button>
<Button variant="secondary">Secondary</Button>
<Button variant="ghost">Ghost</Button>
<Button variant="link">Link</Button>

// Sizes
<Button size="default">Default</Button>
<Button size="sm">Small</Button>
<Button size="lg">Large</Button>
<Button size="icon"><Icon /></Button>

// With loading
<Button disabled>
  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
  Loading
</Button>
```

### Card

```tsx
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  CardFooter,
} from "@/components/ui/card"

<Card>
  <CardHeader>
    <CardTitle>Card Title</CardTitle>
    <CardDescription>Card description here</CardDescription>
  </CardHeader>
  <CardContent>
    <p>Card content goes here</p>
  </CardContent>
  <CardFooter>
    <Button>Action</Button>
  </CardFooter>
</Card>
```

### Dialog

```tsx
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  DialogFooter,
} from "@/components/ui/dialog"

<Dialog>
  <DialogTrigger asChild>
    <Button>Open Dialog</Button>
  </DialogTrigger>
  <DialogContent>
    <DialogHeader>
      <DialogTitle>Dialog Title</DialogTitle>
      <DialogDescription>
        Description of the dialog content
      </DialogDescription>
    </DialogHeader>
    <div>Dialog content here</div>
    <DialogFooter>
      <Button variant="outline">Cancel</Button>
      <Button>Save</Button>
    </DialogFooter>
  </DialogContent>
</Dialog>
```

### Form

```tsx
import { useForm } from "react-hook-form"
import { zodResolver } from "@hookform/resolvers/zod"
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form"
import { Input } from "@/components/ui/input"

const form = useForm<FormValues>({
  resolver: zodResolver(schema)
})

<Form {...form}>
  <form onSubmit={form.handleSubmit(onSubmit)}>
    <FormField
      control={form.control}
      name="nama"
      render={({ field }) => (
        <FormItem>
          <FormLabel>Nama</FormLabel>
          <FormControl>
            <Input placeholder="Masukkan nama" {...field} />
          </FormControl>
          <FormDescription>
            Nama lengkap pekerjaan
          </FormDescription>
          <FormMessage />
        </FormItem>
      )}
    />
    <Button type="submit">Submit</Button>
  </form>
</Form>
```

### Select

```tsx
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

<Select onValueChange={handleChange}>
  <SelectTrigger>
    <SelectValue placeholder="Pilih opsi" />
  </SelectTrigger>
  <SelectContent>
    <SelectItem value="option1">Option 1</SelectItem>
    <SelectItem value="option2">Option 2</SelectItem>
    <SelectItem value="option3">Option 3</SelectItem>
  </SelectContent>
</Select>
```

### Table

```tsx
import {
  Table,
  TableBody,
  TableCaption,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

<Table>
  <TableCaption>Daftar pekerjaan</TableCaption>
  <TableHeader>
    <TableRow>
      <TableHead>No</TableHead>
      <TableHead>Nama</TableHead>
      <TableHead>Pagu</TableHead>
      <TableHead className="text-right">Aksi</TableHead>
    </TableRow>
  </TableHeader>
  <TableBody>
    {data.map((item, index) => (
      <TableRow key={item.id}>
        <TableCell>{index + 1}</TableCell>
        <TableCell>{item.nama}</TableCell>
        <TableCell>{formatCurrency(item.pagu)}</TableCell>
        <TableCell className="text-right">
          <Button size="sm">Edit</Button>
        </TableCell>
      </TableRow>
    ))}
  </TableBody>
</Table>
```

### Tabs

```tsx
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

<Tabs defaultValue="tab1">
  <TabsList>
    <TabsTrigger value="tab1">Tab 1</TabsTrigger>
    <TabsTrigger value="tab2">Tab 2</TabsTrigger>
    <TabsTrigger value="tab3">Tab 3</TabsTrigger>
  </TabsList>
  <TabsContent value="tab1">Content for tab 1</TabsContent>
  <TabsContent value="tab2">Content for tab 2</TabsContent>
  <TabsContent value="tab3">Content for tab 3</TabsContent>
</Tabs>
```

### Toast

```tsx
import { useToast } from "@/components/ui/use-toast"

function MyComponent() {
  const { toast } = useToast()
  
  const handleClick = () => {
    toast({
      title: "Success",
      description: "Data berhasil disimpan",
    })
  }
  
  // Error toast
  toast({
    variant: "destructive",
    title: "Error",
    description: "Terjadi kesalahan",
  })
}
```

---

## 📐 Layout Components

### MainLayout

```tsx
// src/components/layout/MainLayout.tsx
interface MainLayoutProps {
  children: React.ReactNode
}

export function MainLayout({ children }: MainLayoutProps) {
  return (
    <div className="flex h-screen bg-background">
      <Sidebar />
      <div className="flex-1 flex flex-col overflow-hidden">
        <Header />
        <main className="flex-1 overflow-auto p-6">
          {children}
        </main>
      </div>
    </div>
  )
}
```

### Sidebar

```tsx
// src/components/layout/Sidebar.tsx
export function Sidebar() {
  const { menus } = useMenuPermissions()
  const { isCollapsed, toggle } = useSidebarStore()
  
  return (
    <aside className={cn(
      "bg-card border-r transition-all duration-300",
      isCollapsed ? "w-16" : "w-64"
    )}>
      <div className="p-4">
        <Logo collapsed={isCollapsed} />
      </div>
      
      <nav className="px-2">
        {menus.map((menu) => (
          <NavItem key={menu.key} menu={menu} />
        ))}
      </nav>
    </aside>
  )
}
```

### Header

```tsx
// src/components/layout/Header.tsx
export function Header() {
  const { user, logout } = useAuth()
  
  return (
    <header className="h-16 border-b bg-card px-6 flex items-center justify-between">
      <div className="flex items-center gap-4">
        <BreadcrumbNav />
      </div>
      
      <div className="flex items-center gap-4">
        <ThemeToggle />
        <NotificationBell />
        <UserMenu user={user} onLogout={logout} />
      </div>
    </header>
  )
}
```

---

## 🔧 Custom Components

### DataTable

```tsx
// src/components/ui/data-table.tsx
interface DataTableProps<TData> {
  columns: ColumnDef<TData>[]
  data: TData[]
  searchKey?: string
  onRowClick?: (row: TData) => void
}

export function DataTable<TData>({
  columns,
  data,
  searchKey,
  onRowClick
}: DataTableProps<TData>) {
  const table = useReactTable({
    data,
    columns,
    getCoreRowModel: getCoreRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
  })
  
  return (
    <div>
      {searchKey && (
        <Input
          placeholder="Search..."
          onChange={(e) => table.setGlobalFilter(e.target.value)}
          className="max-w-sm mb-4"
        />
      )}
      
      <Table>
        {/* Table content */}
      </Table>
      
      <DataTablePagination table={table} />
    </div>
  )
}
```

### ConfirmDialog

```tsx
// src/components/ui/confirm-dialog.tsx
interface ConfirmDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: string
  onConfirm: () => void
  confirmText?: string
  cancelText?: string
  variant?: 'default' | 'destructive'
}

export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  onConfirm,
  confirmText = "Confirm",
  cancelText = "Cancel",
  variant = "default"
}: ConfirmDialogProps) {
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{cancelText}</AlertDialogCancel>
          <AlertDialogAction
            onClick={onConfirm}
            className={variant === 'destructive' ? 'bg-destructive' : ''}
          >
            {confirmText}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
```

### LoadingSpinner

```tsx
// src/components/ui/loading-spinner.tsx
interface LoadingSpinnerProps {
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

export function LoadingSpinner({ size = 'md', className }: LoadingSpinnerProps) {
  const sizeClasses = {
    sm: 'h-4 w-4',
    md: 'h-8 w-8',
    lg: 'h-12 w-12'
  }
  
  return (
    <Loader2 className={cn(
      "animate-spin text-primary",
      sizeClasses[size],
      className
    )} />
  )
}
```

### PageHeader

```tsx
// src/components/ui/page-header.tsx
interface PageHeaderProps {
  title: string
  description?: string
  actions?: React.ReactNode
}

export function PageHeader({ title, description, actions }: PageHeaderProps) {
  return (
    <div className="flex items-center justify-between mb-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{title}</h1>
        {description && (
          <p className="text-muted-foreground">{description}</p>
        )}
      </div>
      {actions && <div className="flex gap-2">{actions}</div>}
    </div>
  )
}
```

---

## 🎭 Component Variants

### Badge Variants

```tsx
import { Badge } from "@/components/ui/badge"

<Badge variant="default">Default</Badge>
<Badge variant="secondary">Secondary</Badge>
<Badge variant="destructive">Destructive</Badge>
<Badge variant="outline">Outline</Badge>

// Custom status badges
<Badge className="bg-green-500">Selesai</Badge>
<Badge className="bg-yellow-500">Proses</Badge>
<Badge className="bg-red-500">Terlambat</Badge>
```

### Alert Variants

```tsx
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { AlertCircle, CheckCircle, Info, AlertTriangle } from "lucide-react"

// Info
<Alert>
  <Info className="h-4 w-4" />
  <AlertTitle>Info</AlertTitle>
  <AlertDescription>Informasi penting</AlertDescription>
</Alert>

// Success
<Alert className="border-green-500">
  <CheckCircle className="h-4 w-4 text-green-500" />
  <AlertTitle>Sukses</AlertTitle>
  <AlertDescription>Operasi berhasil</AlertDescription>
</Alert>

// Warning
<Alert className="border-yellow-500">
  <AlertTriangle className="h-4 w-4 text-yellow-500" />
  <AlertTitle>Peringatan</AlertTitle>
  <AlertDescription>Harap perhatikan</AlertDescription>
</Alert>

// Error
<Alert variant="destructive">
  <AlertCircle className="h-4 w-4" />
  <AlertTitle>Error</AlertTitle>
  <AlertDescription>Terjadi kesalahan</AlertDescription>
</Alert>
```

---

## 📚 Related Documentation

- [Architecture](./ARCHITECTURE.md)
- [Features](./FEATURES.md)
- [Installation Guide](./INSTALLATION.md)
