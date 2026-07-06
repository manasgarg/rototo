// The contract proof (design/console-surfaces.md, C6 gate): the in-repo
// experiences hold zero architectural privilege, which is only true if they
// touch zero private APIs. Every module under src/extensions/<name>/ may
// import exactly three things: react, the extension contract
// (src/extension-api.ts), and its own extension's files. Anything else
// fails this check, and with it the build.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const extensionsRoot = path.join(root, "src", "extensions");
const contractPath = path.join(root, "src", "extension-api.ts");

const IMPORT_PATTERN =
    /(?:import|export)\s+[^"'`]*?from\s*["']([^"']+)["']|import\s*["']([^"']+)["']/g;

const failures = [];
const extensions = readdirSync(extensionsRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name);

if (extensions.length === 0) {
    console.error("no extensions found under src/extensions");
    process.exit(1);
}

for (const extension of extensions) {
    const dir = path.join(extensionsRoot, extension);
    const files = readdirSync(dir, {
        withFileTypes: true,
        recursive: true,
    })
        .filter(
            (entry) =>
                entry.isFile() &&
                (entry.name.endsWith(".ts") || entry.name.endsWith(".tsx")) &&
                // Unit tests run under node and never ship in the bundle;
                // the contract binds what the console composes in.
                !entry.name.endsWith(".test.ts"),
        )
        .map((entry) => path.join(entry.parentPath, entry.name));
    for (const file of files) {
        const source = readFileSync(file, "utf8");
        for (const match of source.matchAll(IMPORT_PATTERN)) {
            const specifier = (match[1] ?? match[2] ?? "").trim();
            if (specifier === "") {
                continue;
            }
            if (specifier === "react" || specifier === "react/jsx-runtime") {
                continue;
            }
            if (specifier.startsWith(".")) {
                const resolved = path.resolve(path.dirname(file), specifier);
                if (resolved === contractPath) {
                    continue;
                }
                if (
                    (resolved + path.sep).startsWith(dir + path.sep) ||
                    resolved === dir
                ) {
                    continue;
                }
                failures.push(
                    `${path.relative(root, file)}: imports "${specifier}", which escapes the extension`,
                );
                continue;
            }
            failures.push(
                `${path.relative(root, file)}: imports "${specifier}"; extensions may import only react and the contract`,
            );
        }
    }
}

if (failures.length > 0) {
    console.error("extension contract violations:");
    for (const failure of failures) {
        console.error(`  ${failure}`);
    }
    process.exit(1);
}
console.log(
    `extension contract holds: ${extensions.join(", ")} import only react and the contract`,
);
