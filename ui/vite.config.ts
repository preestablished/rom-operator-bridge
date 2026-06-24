import { defineConfig } from "vite";

export const SPA_RESPONSE_HEADERS = {
  "Cache-Control": "no-store",
  Pragma: "no-cache",
  "Content-Security-Policy":
    "default-src 'self'; connect-src 'self' wss://rombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'",
  "Referrer-Policy": "no-referrer",
  "X-Frame-Options": "DENY",
  "X-Content-Type-Options": "nosniff"
} as const;

export default defineConfig({
  appType: "spa",
  publicDir: "public",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true
  },
  preview: {
    host: "127.0.0.1",
    port: 4173,
    strictPort: true,
    headers: SPA_RESPONSE_HEADERS
  }
});
