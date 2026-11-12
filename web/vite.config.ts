import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { fileURLToPath } from 'node:url';

// The API the dev server proxies to. The local server by default; set AOS_PROXY for another.
const target = process.env.AOS_PROXY || 'http://127.0.0.1:8787';

export default defineConfig({
  // Relative paths: the same build runs from the Rust server at `/`, from a Capacitor shell
  // (capacitor://localhost) or from a file server.
  base: './',
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  server: {
    host: true,
    port: Number(process.env.PORT || 5190),
    strictPort: true,
    proxy: {
      '/api': { target, changeOrigin: true, ws: true },
      '/mcp': { target, changeOrigin: true },
    },
  },
  preview: { host: true, port: 5191 },
  build: {
    target: ['es2022', 'safari16'],
    sourcemap: false,
    chunkSizeWarningLimit: 900,
  },
});
