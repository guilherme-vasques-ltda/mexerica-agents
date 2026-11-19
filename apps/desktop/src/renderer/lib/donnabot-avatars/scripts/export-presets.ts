// Gera presets/roles.json a partir de src/core/data.ts
// Uso: node --experimental-strip-types scripts/export-presets.ts
import { writeFileSync, mkdirSync } from 'node:fs';
import { ROLES, PALETTE } from '../src/core/data.ts';

mkdirSync(new URL('../presets/', import.meta.url), { recursive: true });
writeFileSync(new URL('../presets/roles.json', import.meta.url), JSON.stringify(ROLES, null, 2) + '\n');
writeFileSync(new URL('../presets/palette.json', import.meta.url), JSON.stringify(PALETTE, null, 2) + '\n');
console.log(`ok: ${ROLES.length} presets, ${PALETTE.length} cores`);
