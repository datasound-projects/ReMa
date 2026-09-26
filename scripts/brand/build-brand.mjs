// Generates every ReMa logo file from the master app icon.
//
//   node scripts/brand/build-brand.mjs      (pnpm brand)
//
// The master is src/assets/brand/rema-icon-source.png: the glossy blue tile
// with the white R and its blue node, full-bleed, 1024 × 1024, transparent
// outside the rounded corners. From it this writes:
//   - src-tauri/icons/*          desktop app icons (macOS, Windows, Linux),
//                                on the macOS icon grid with a soft shadow;
//   - src/assets/brand/rema-icon.png            the in-app icon (256 px);
//   - src/assets/brand/rema-logo-{light,dark}.svg  icon + wordmark lockups.
// Resizing is done by `tauri icon`, so no image library is needed.

import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '../..');
const brand = join(root, 'src/assets/brand');
const source = join(brand, 'rema-icon-source.png');
const wordmark = JSON.parse(readFileSync(join(here, 'wordmark.json'), 'utf8'));

const dataUri = (file) => `data:image/png;base64,${readFileSync(file).toString('base64')}`;

function tauriIcon(input, output, extra = []) {
  const run = spawnSync('pnpm', ['exec', 'tauri', 'icon', input, '--output', output, ...extra], {
    cwd: root,
    encoding: 'utf8',
    shell: process.platform === 'win32',
  });
  if (run.status !== 0) {
    process.stderr.write(run.stderr ?? '');
    process.exit(run.status ?? 1);
  }
}

/**
 * The desktop icon: the tile at 824 px on a 1024 px canvas (the macOS icon
 * grid, so it sits with other apps in the Dock and taskbar) over a soft
 * shadow.
 */
const desktopIcon = (href) => `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <filter id="shadow" x="-10%" y="-10%" width="120%" height="125%">
      <feDropShadow dx="0" dy="12" stdDeviation="16" flood-color="#021633" flood-opacity="0.3"/>
    </filter>
  </defs>
  <image x="100" y="100" width="824" height="824" filter="url(#shadow)" href="${href}" xlink:href="${href}"/>
</svg>
`;

/** The icon followed by the "ReMa" wordmark (Inter, outlined), on one line. */
function lockup(href, ink) {
  const size = 120;
  const capHeight = size * 0.42;
  const scale = capHeight / wordmark.capHeight;
  const x = size + size * 0.26;
  const baseline = size / 2 + capHeight / 2;
  const width = Math.ceil(x + wordmark.advance * scale);
  return `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 ${width} ${size}">
  <image width="${size}" height="${size}" href="${href}" xlink:href="${href}"/>
  <path transform="translate(${x.toFixed(2)} ${baseline.toFixed(2)}) scale(${scale.toFixed(5)})" fill="${ink}" d="${wordmark.d}"/>
</svg>
`;
}

const temp = mkdtempSync(join(tmpdir(), 'rema-brand-'));
try {
  // Desktop icons: only the files the bundle uses (`tauri icon` also makes
  // store and mobile icons ReMa doesn't ship).
  const desktopSvg = join(temp, 'desktop.svg');
  writeFileSync(desktopSvg, desktopIcon(dataUri(source)));
  const desktopOut = join(temp, 'desktop');
  tauriIcon(desktopSvg, desktopOut);
  const icons = ['32x32.png', '128x128.png', '128x128@2x.png', 'icon.icns', 'icon.ico', 'icon.png'];
  for (const file of icons) copyFileSync(join(desktopOut, file), join(root, 'src-tauri/icons', file));

  // The in-app icon: sharp up to 64 px on 4× screens (title bar, launch intro, About).
  const appOut = join(temp, 'app');
  tauriIcon(source, appOut, ['--png', '256']);
  const inApp = join(brand, 'rema-icon.png');
  copyFileSync(join(appOut, '256x256.png'), inApp);

  // Lockups for use outside the app (website, documents).
  writeFileSync(join(brand, 'rema-logo-light.svg'), lockup(dataUri(inApp), '#191919'));
  writeFileSync(join(brand, 'rema-logo-dark.svg'), lockup(dataUri(inApp), '#ecebe6'));

  console.log(`Updated ${icons.length} desktop icons, rema-icon.png and the logo lockups`);
} finally {
  rmSync(temp, { recursive: true, force: true });
}
