---
description: Fix code style with Laravel Pint
---

# Code Style Workflow

// turbo-all

## Run Laravel Pint

```bash
./vendor/bin/pint
```

## Check Without Fixing (Dry Run)

```bash
./vendor/bin/pint --test
```

## Fix Specific File

```bash
./vendor/bin/pint app/Http/Controllers/PekerjaanController.php
```

## Fix Specific Directory

```bash
./vendor/bin/pint app/Models
```

## Verbose Output

```bash
./vendor/bin/pint -v
```

## Configuration

Pint uses Laravel's preset by default. To customize, create `pint.json` in project root:

```json
{
    "preset": "laravel",
    "rules": {
        "array_syntax": {
            "syntax": "short"
        }
    }
}
```

## Notes

- Run Pint before committing code
- Pint follows PSR-12 with Laravel conventions
- Can be integrated with pre-commit hooks
