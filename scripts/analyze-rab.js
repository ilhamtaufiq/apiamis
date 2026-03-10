import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const { analyzeRAB } = require('@ilhamtaufiq/rab-analyzer');
import path from 'path';
import fs from 'fs';

const filePath = process.argv[2];

if (!filePath) {
    console.error('Error: No file path provided.');
    process.exit(1);
}

try {
    const absolutePath = path.resolve(filePath);
    if (!fs.existsSync(absolutePath)) {
        console.error(`Error: File not found at ${absolutePath}`);
        process.exit(1);
    }

    const outputPath = await analyzeRAB(absolutePath);
    console.log(outputPath);
} catch (error) {
    console.error('Extraction failed:', error.message);
    process.exit(1);
}
