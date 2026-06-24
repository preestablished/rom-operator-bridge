import { defineConfig } from "vite";

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
  }
});
