import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath, URL } from 'node:url'
// The import attribute is required by Vite's native config loader.
import pkg from './package.json' with { type: 'json' }

// The built app is compiled into the austeris binary and served by its
// gateway (ADR 0011), so `dist` is what `rust-embed` picks up.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  build: {
    // One bundle of about 250 kB gzipped, served from the installation itself
    // and cached as immutable (its name changes with its content). Splitting
    // it buys a first load on a phone a few hundred milliseconds and every
    // later load nothing, so the warning is set above what this app is.
    chunkSizeWarningLimit: 1000,
  },
  // The session is a cookie, so the API has to be same-origin. In development
  // Vite forwards `/api` to austeris - `docker compose up` publishes it on
  // 8084, and AUSTERIS_DEV_API points elsewhere - and in production the
  // gateway serves this build itself.
  server: {
    port: 5173,
    strictPort: true,
    proxy: { '/api': process.env.AUSTERIS_DEV_API ?? 'http://127.0.0.1:8084' },
  },
})
