// Generates every ReMa logo variant from one master mark.
//
//   node scripts/brand/build-brand.mjs
//
// The mark is an abstract R made of two forms that meet at a node:
//   - the loop (stem and bowl): searching, the profile, "you";
//   - the path (the leg): moving forward, the next role;
//   - the node, held in a small clearing where they meet: the match.
// Every variant uses the same geometry; only color, lighting and scale change.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '../../src/assets/brand');

/** The master mark and its palettes (shared with the app's BrandMark). */
const MARK = JSON.parse(readFileSync(join(out, 'mark.json'), 'utf8'));
const P = MARK.palettes;

const wordmark = JSON.parse(readFileSync(join(here, 'wordmark.json'), 'utf8'));

/** The two forms and the node; `paint` gives each part's paint. */
function forms(id, paint, { filter = '', gloss = false } = {}) {
  const { stroke, loop, path, node, clearing } = MARK;
  const glossLayer = gloss
    ? `<path d="${loop}" stroke="url(#${id}-gloss)"/><path d="${path}" stroke="url(#${id}-gloss)"/>`
    : '';
  return `
  <mask id="${id}-clear" maskUnits="userSpaceOnUse" x="0" y="0" width="120" height="120">
    <rect width="120" height="120" fill="#fff"/>
    <circle cx="${node.cx}" cy="${node.cy}" r="${clearing}" fill="#000"/>
  </mask>
  <g${filter ? ` filter="url(#${filter})"` : ''}>
    <g mask="url(#${id}-clear)" fill="none" stroke-width="${stroke}" stroke-linecap="round" stroke-linejoin="round">
      <path d="${loop}" stroke="${paint.loop}"/>
      <path d="${path}" stroke="${paint.path}"/>
      ${glossLayer}
    </g>
    <circle cx="${node.cx}" cy="${node.cy}" r="${node.r}" fill="${paint.node}"/>
  </g>`;
}

const svg = (viewBox, body, size) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}"${size ? ` width="${size}" height="${size}"` : ''}>${body}\n</svg>\n`;

/** Gradients shared by the lit variants (user space, so strokes map cleanly). */
function gradients(id, c) {
  return `
  <defs>
    <linearGradient id="${id}-loop" gradientUnits="userSpaceOnUse" x1="28" y1="20" x2="92" y2="100">
      <stop offset="0" stop-color="${c.loop[0]}"/><stop offset="1" stop-color="${c.loop[1]}"/>
    </linearGradient>
    <linearGradient id="${id}-path" gradientUnits="userSpaceOnUse" x1="58" y1="68" x2="92" y2="100">
      <stop offset="0" stop-color="${c.path[0]}"/><stop offset="1" stop-color="${c.path[1]}"/>
    </linearGradient>
    <linearGradient id="${id}-gloss" gradientUnits="userSpaceOnUse" x1="0" y1="18" x2="0" y2="62">
      <stop offset="0" stop-color="#fff" stop-opacity="${c.gloss}"/><stop offset="1" stop-color="#fff" stop-opacity="0"/>
    </linearGradient>
    ${c.glow ? `<filter id="${id}-glow" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="3.2" result="blur"/>
      <feFlood flood-color="${c.glow}" flood-opacity="0.45"/>
      <feComposite in2="blur" operator="in" result="glow"/>
      <feMerge><feMergeNode in="glow"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>` : ''}
  </defs>`;
}

const lit = (id, c) =>
  svg(
    '0 0 120 120',
    gradients(id, c) +
      forms(id, { loop: `url(#${id}-loop)`, path: `url(#${id}-path)`, node: c.node }, {
        filter: c.glow ? `${id}-glow` : '',
        gloss: true,
      }),
  );

// Variant F — light surfaces: deeper blues, crisp, no glow.
const LIGHT = P.light;
// Variant E — dark surfaces: brighter blues, edge light, a controlled glow.
const DARK = P.dark;

const flat = (c) => svg('0 0 120 120', forms('f', c));
const mono = (color) => svg('0 0 120 120', forms('m', { loop: color, path: color, node: color }));

/** Variant C — the mark followed by the wordmark, centered on one line. */
function lockup(markSvg, ink) {
  const { bounds } = MARK;
  // Optically matched: the mark stands a little taller than the capitals.
  const capHeight = bounds.height / 1.14;
  const scale = capHeight / wordmark.capHeight;
  const centerY = bounds.y + bounds.height / 2;
  const baseline = centerY + capHeight / 2;
  const x = bounds.x + bounds.width + bounds.width * 0.36;
  const pad = 6;
  const left = bounds.x - pad;
  const width = Math.ceil(x + wordmark.advance * scale + pad - left);
  const inner = markSvg.replace(/^<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '');
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${left} ${bounds.y - pad} ${width} ${bounds.height + 2 * pad}">
  <g>${inner}</g>
  <path transform="translate(${x.toFixed(2)} ${baseline.toFixed(2)}) scale(${scale.toFixed(5)})" fill="${ink}" d="${wordmark.d}"/>
</svg>
`;
}

/** Variant A — the app icon: a glassy blue tile (macOS grid) with a light mark. */
function appIcon() {
  const s = 5.8; // mark scale inside the 824 px tile
  const { bounds } = MARK;
  const tx = 512 - (bounds.x + bounds.width / 2) * s;
  const ty = 512 - (bounds.y + bounds.height / 2) * s;
  const tile = 'x="100" y="100" width="824" height="824" rx="185"';
  const mark = forms('a', { loop: 'url(#a-loop)', path: 'url(#a-path)', node: '#ffffff' }, { filter: 'a-glyph', gloss: false });
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <linearGradient id="a-tile" x1="0.2" y1="0" x2="0.8" y2="1">
      <stop offset="0" stop-color="#2a8cf0"/>
      <stop offset="0.45" stop-color="#0a66c2"/>
      <stop offset="1" stop-color="#062f66"/>
    </linearGradient>
    <radialGradient id="a-light" cx="0.3" cy="0.18" r="0.75">
      <stop offset="0" stop-color="#8fd2ff" stop-opacity="0.4"/>
      <stop offset="0.5" stop-color="#3aa0ff" stop-opacity="0.1"/>
      <stop offset="1" stop-color="#3aa0ff" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="a-depth" cx="0.5" cy="0.45" r="0.7">
      <stop offset="0.62" stop-color="#021633" stop-opacity="0"/>
      <stop offset="1" stop-color="#021633" stop-opacity="0.42"/>
    </radialGradient>
    <linearGradient id="a-sheen" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#fff" stop-opacity="0.18"/>
      <stop offset="1" stop-color="#fff" stop-opacity="0"/>
    </linearGradient>
    <linearGradient id="a-rim" x1="0.2" y1="0" x2="0.8" y2="1">
      <stop offset="0" stop-color="#fff" stop-opacity="0.6"/>
      <stop offset="0.35" stop-color="#bfe3ff" stop-opacity="0.22"/>
      <stop offset="1" stop-color="#021a3d" stop-opacity="0.5"/>
    </linearGradient>
    <linearGradient id="a-loop" gradientUnits="userSpaceOnUse" x1="28" y1="20" x2="92" y2="100">
      <stop offset="0" stop-color="#ffffff"/><stop offset="1" stop-color="#e3f1ff"/>
    </linearGradient>
    <linearGradient id="a-path" gradientUnits="userSpaceOnUse" x1="58" y1="68" x2="92" y2="100">
      <stop offset="0" stop-color="#d9f3ff"/><stop offset="1" stop-color="#8fd6ff"/>
    </linearGradient>
    <filter id="a-glyph" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="1.2" stdDeviation="1.6" flood-color="#021633" flood-opacity="0.35"/>
      <feDropShadow dx="0" dy="0" stdDeviation="2.4" flood-color="#8fd6ff" flood-opacity="0.35"/>
    </filter>
    <filter id="a-shadow" x="-10%" y="-10%" width="120%" height="125%">
      <feDropShadow dx="0" dy="14" stdDeviation="18" flood-color="#021633" flood-opacity="0.32"/>
    </filter>
  </defs>
  <g filter="url(#a-shadow)">
    <rect ${tile} fill="url(#a-tile)"/>
    <rect ${tile} fill="url(#a-light)"/>
    <rect ${tile} fill="url(#a-depth)"/>
    <path d="M100 285C100 183 183 100 285 100H739C841 100 924 183 924 285V400C790 450 640 470 512 462C370 454 230 410 100 440Z" fill="url(#a-sheen)"/>
    <rect ${tile} fill="none" stroke="url(#a-rim)" stroke-width="3"/>
  </g>
  <g transform="translate(${tx.toFixed(1)} ${ty.toFixed(1)}) scale(${s})">${mark}
  </g>
</svg>
`;
}

const files = {
  // A — primary app icon (source for the desktop icon set).
  'rema-app-icon.svg': appIcon(),
  // B — in-app navigation icon: flat, two tones, for 16–24 px.
  'rema-nav-light.svg': flat(P.navLight),
  'rema-nav-dark.svg': flat(P.navDark),
  // C — mark + wordmark.
  'rema-logo-light.svg': lockup(lit('l', LIGHT), '#191919'),
  'rema-logo-dark.svg': lockup(lit('d', DARK), '#ecebe6'),
  // D — one color.
  'rema-mark-white.svg': mono(P.mono.white),
  'rema-mark-black.svg': mono(P.mono.black),
  'rema-mark-blue.svg': mono(P.mono.blue),
  // E / F — the mark for dark and light surfaces.
  'rema-mark-dark.svg': lit('d', DARK),
  'rema-mark-light.svg': lit('l', LIGHT),
};

for (const [name, content] of Object.entries(files)) {
  writeFileSync(join(out, name), content);
}
console.log(`wrote ${Object.keys(files).length} files to ${out}`);
