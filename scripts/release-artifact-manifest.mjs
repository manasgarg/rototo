#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readdir, readFile, stat, writeFile, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { dirname, join, relative } from "node:path";

const version = process.argv[2];
const output = process.argv[3] || "target/release-artifacts/manifest.json";
if (!version) {
  console.error("usage: node scripts/release-artifact-manifest.mjs <version> [output]");
  process.exit(1);
}

const roots = [
  "target/package",
  "sdks/python/dist",
  "sdks/typescript",
  "sdks/java/target",
  "apps/console-server",
];
const suffixes = [".crate", ".tar.gz", ".whl", ".tgz", ".jar", ".pom", ".node"];
const artifacts = [];

for (const root of roots) {
  if (existsSync(root)) {
    await collect(root);
  }
}

artifacts.sort((left, right) => left.path.localeCompare(right.path));
await mkdir(dirname(output), { recursive: true });
await writeFile(
  output,
  `${JSON.stringify({
    version,
    generatedAt: new Date().toISOString(),
    artifacts,
    registryLinks: {
      crates: `https://crates.io/crates/rototo/${version}`,
      pypi: `https://pypi.org/project/rototo/${pythonVersion(version)}/`,
      npm: `https://www.npmjs.com/package/rototo/v/${version}`,
      console: `https://www.npmjs.com/package/@rototo/console/v/${version}`,
      maven: `https://central.sonatype.com/artifact/dev.rototo/rototo/${version}`,
      go: `https://pkg.go.dev/github.com/manasgarg/rototo/sdks/go@v${version}`,
    },
  }, null, 2)}\n`,
);
console.log(`wrote ${output}`);

async function collect(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules") {
      continue;
    }
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      await collect(path);
    } else if (suffixes.some((suffix) => entry.name.endsWith(suffix))) {
      const bytes = await readFile(path);
      const info = await stat(path);
      artifacts.push({
        path: relative(process.cwd(), path),
        bytes: info.size,
        sha256: createHash("sha256").update(bytes).digest("hex"),
      });
    }
  }
}

function pythonVersion(canonical) {
  return canonical.replace(/-alpha\.(\d+)$/, "a$1");
}
