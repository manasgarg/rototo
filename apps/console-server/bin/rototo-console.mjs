#!/usr/bin/env node
// The rototo-console executable: the server entry, which also serves the
// built web app. The published package carries compiled JavaScript in
// dist/ because Node refuses to type-strip TypeScript under node_modules;
// a repo checkout falls back to running the sources directly.
import { existsSync } from "node:fs";

const dist = new URL("../dist/main.js", import.meta.url);
await (existsSync(dist) ? import(dist.href) : import("../src/main.ts"));
