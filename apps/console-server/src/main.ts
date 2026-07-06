// Dev entry point for the new console server. How this ships inside or
// beside `rototo console` is the C7 product-shape decision; until then the
// server runs behind a dev flag: `npm run dev -- [--port N] [--data-dir D]`.

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
    },
});

const config = resolveConfig(process.env, {
    port: values.port !== undefined ? Number(values.port) : undefined,
    host: values.host,
    dataDir: values["data-dir"] ?? null,
    publicUrl: values["public-url"],
});

const store = new Store(config.dataDir);
const app = buildApp({ config, store, github: new GitHubApi() });

// Every few minutes, sooner when nudged (the nudge is the reconcile route;
// webhooks arrive with Phase B).
app.reconciler.start(120_000);

serve({ fetch: app.fetch, hostname: config.host, port: config.port }, () => {
    console.log(
        `rototo console server (${config.authMode} mode) on http://${config.host}:${config.port}`,
    );
});
