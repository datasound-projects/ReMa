/// <reference types="vitest/config" />
import { createReadStream, readdirSync, readFileSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, extname, join, normalize, sep } from 'node:path';

import react from '@vitejs/plugin-react';
import { defineConfig, type Plugin } from 'vite';

// Set by the Tauri CLI when developing against a remote device.
const host = process.env.TAURI_DEV_HOST;
const isDebugBuild = Boolean(process.env.TAURI_ENV_DEBUG);

/**
 * PDF.js loads its standard fonts, character maps, color profiles and
 * image decoders at runtime. They are served from ReMa itself under
 * `/pdfjs/` (never from a CDN): copied into the build, and served by the
 * dev server.
 */
function pdfjsAssets(): Plugin {
  const root = dirname(createRequire(import.meta.url).resolve('pdfjs-dist/package.json'));
  const folders = ['cmaps', 'standard_fonts', 'iccs', 'wasm'];
  const types: Record<string, string> = {
    '.bcmap': 'application/octet-stream',
    '.pfb': 'application/octet-stream',
    '.ttf': 'font/ttf',
    '.icc': 'application/vnd.iccprofile',
    '.wasm': 'application/wasm',
    '.js': 'text/javascript',
  };
  const files = folders.flatMap((folder) =>
    readdirSync(join(root, folder))
      // quickjs-eval runs PDF scripts, which ReMa never does.
      .filter((name) => extname(name) in types && !name.startsWith('quickjs'))
      .map((name) => `${folder}/${name}`),
  );
  return {
    name: 'rema-pdfjs-assets',
    configureServer(server) {
      server.middlewares.use('/pdfjs/', (req, res, next) => {
        const path = normalize(decodeURIComponent((req.url ?? '').split('?')[0] ?? '')).replace(/^[/\\]+/, '');
        if (!files.includes(path.split(sep).join('/'))) return next();
        const file = join(root, path);
        res.setHeader('Content-Type', types[extname(file)] ?? 'application/octet-stream');
        res.setHeader('Content-Length', statSync(file).size);
        createReadStream(file).pipe(res);
      });
    },
    generateBundle() {
      for (const path of files) {
        this.emitFile({ type: 'asset', fileName: `pdfjs/${path}`, source: readFileSync(join(root, path)) });
      }
    },
  };
}

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react(), pdfjsAssets()],

  // Keep Rust compiler output visible in the terminal.
  clearScreen: false,

  server: {
    // Tauri expects a fixed port (see src-tauri/tauri.conf.json -> build.devUrl).
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      // Cargo handles the Rust side; don't reload the UI on backend edits.
      ignored: ['**/src-tauri/**'],
    },
  },

  // Expose TAURI_ENV_* variables to the frontend alongside VITE_*.
  envPrefix: ['VITE_', 'TAURI_ENV_'],

  build: {
    // WebView2 (Windows) is Chromium-based; macOS and Linux use WebKit.
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari15',
    minify: !isDebugBuild,
    sourcemap: isDebugBuild,
    // PDF.js and pdfmake are large but load only when a PDF is shown or made.
    chunkSizeWarningLimit: 1100,
  },

  test: {
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    environment: 'node',
  },
});
