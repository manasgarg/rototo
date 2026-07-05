import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The new console frontend. In development, Vite proxies /api to the
// TypeScript console server (apps/console-server, default 127.0.0.1:7687).
// Port 5174 keeps it runnable beside the old console's dev server on 5173
// until cutover.
export default defineConfig({
    plugins: [react()],
    resolve: {
        alias: {
            "@": fileURLToPath(new URL("./src", import.meta.url)),
        },
    },
    server: {
        host: "127.0.0.1",
        port: 5174,
        strictPort: true,
        proxy: {
            "/api": {
                target:
                    process.env.ROTOTO_CONSOLE_API ?? "http://127.0.0.1:7687",
                changeOrigin: false,
            },
        },
    },
    build: {
        outDir: "dist",
        emptyOutDir: true,
    },
});
