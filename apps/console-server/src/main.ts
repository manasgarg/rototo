// The console's entry point. The console ships as its own product (the C7
// product-shape decision): this server serves the JSON API and the built
// web app from one process, with no rototo CLI anywhere in the path. Run
// it with `npx rototo-console` (or `npm run dev` in the repo).

import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";

import { serve } from "@hono/node-server";

import { buildApp } from "./app.ts";
import { resolveConfig } from "./config.ts";
import { GitHubApi } from "./github.ts";
import { Store } from "./store.ts";

const { values } = parseArgs({
    options: {
        port: { type: "string" },
        host: { type: "string" },
        "data-dir": { type: "string" },
        "public-url": { type: "string" },
        web: { type: "string" },
    },
});

const config = resolveConfig(process.env, {
    port: values.port !== undefined ? Number(values.port) : undefined,
    host: values.host,
    dataDir: values["data-dir"],
    publicUrl: values["public-url"],
});

const store = new Store(config.dataDir);
const app = buildApp({ config, store, github: new GitHubApi() });

// Every few minutes, sooner when nudged (the reconcile route, or a GitHub
// webhook).
app.reconciler.start(120_000);

// The built web app, when present: --web, then the packaged copy, then the
// repo checkout's dist. Absent means API-only, which is a fine way to run
// behind a separate static host.
const webRoot = [
    values.web,
    process.env.ROTOTO_CONSOLE_WEB_DIST,
    path.resolve(import.meta.dirname, "../web"),
    path.resolve(import.meta.dirname, "../../console-web/dist"),
].find((candidate) => candidate !== undefined && existsSync(candidate));

const TYPES: Record<string, string> = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".svg": "image/svg+xml",
    ".png": "image/png",
    ".ico": "image/x-icon",
    ".woff2": "font/woff2",
    ".map": "application/json",
};

function serveWeb(pathname: string): Response | null {
    if (webRoot === undefined) {
        return null;
    }
    const relative = pathname === "/" ? "index.html" : pathname.slice(1);
    let file = path.resolve(webRoot, relative);
    // Anything outside the web root, and any route the SPA owns, gets
    // index.html; the hash router takes it from there.
    if (!file.startsWith(webRoot + path.sep) || !existsSync(file)) {
        file = path.join(webRoot, "index.html");
        if (!existsSync(file)) {
            return null;
        }
    }
    return new Response(readFileSync(file), {
        headers: {
            "content-type":
                TYPES[path.extname(file)] ?? "application/octet-stream",
        },
    });
}

serve(
    {
        fetch: (request: Request) => {
            const url = new URL(request.url);
            if (!url.pathname.startsWith("/api")) {
                const page = serveWeb(url.pathname);
                if (page !== null) {
                    return page;
                }
            }
            return app.fetch(request);
        },
        hostname: config.host,
        port: config.port,
    },
    () => {
        console.log(
            `rototo console (${config.authMode} mode) on http://${config.host}:${config.port}${webRoot === undefined ? " (API only; no web bundle found)" : ""}`,
        );
    },
);
