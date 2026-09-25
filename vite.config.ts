import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Set by the Tauri CLI when developing against a remote device.
const host = process.env.TAURI_DEV_HOST;
const isDebugBuild = Boolean(process.env.TAURI_ENV_DEBUG);

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react()],

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
  },
});
