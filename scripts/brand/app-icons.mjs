// Regenerates the desktop app icons from the primary app icon (variant A).
//
//   node scripts/brand/app-icons.mjs
//
// `tauri icon` renders src/assets/brand/rema-app-icon.svg at every size into a
// temporary folder; only the files the bundle uses are copied to
// src-tauri/icons (it also makes store and mobile icons ReMa doesn't ship).

import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '../..');
const source = join(root, 'src/assets/brand/rema-app-icon.svg');
const target = join(root, 'src-tauri/icons');
const files = ['32x32.png', '128x128.png', '128x128@2x.png', 'icon.icns', 'icon.ico', 'icon.png'];

const temp = mkdtempSync(join(tmpdir(), 'rema-icons-'));
try {
  const run = spawnSync('pnpm', ['exec', 'tauri', 'icon', source, '--output', temp], {
    cwd: root,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });
  if (run.status !== 0) process.exit(run.status ?? 1);
  for (const file of files) copyFileSync(join(temp, file), join(target, file));
  console.log(`Updated ${files.length} icons in src-tauri/icons`);
} finally {
  rmSync(temp, { recursive: true, force: true });
}
