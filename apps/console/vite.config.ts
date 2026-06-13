import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The console frontend builds to static assets that the `rototo console`
// server embeds and serves. In development, Vite proxies /api to a locally
// running `rototo console` (default bind 127.0.0.1:7686).
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    proxy: {
      "/api": {
        target: process.env.ROTOTO_CONSOLE_API ?? "http://127.0.0.1:7686",
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
