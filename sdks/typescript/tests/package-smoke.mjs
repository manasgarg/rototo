import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const sdk = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(sdk, "../..");
const temp = mkdtempSync(join(tmpdir(), "rototo-typescript-package-"));

try {
  const tarballName = execFileSync("npm", ["pack", "--pack-destination", temp], {
    cwd: sdk,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  })
    .trim()
    .split(/\r?\n/)
    .at(-1);
  const tarball = join(temp, tarballName);

  execFileSync("npm", ["init", "-y"], {
    cwd: temp,
    stdio: "ignore",
  });
  execFileSync("npm", ["install", tarball], {
    cwd: temp,
    stdio: "inherit",
  });

  const script = `
    import { Workspace, __version__ } from "rototo";
    if (!/^\\d+\\.\\d+\\.\\d+(-[0-9A-Za-z.-]+)?$/.test(__version__)) {
      throw new Error("unexpected version " + __version__);
    }
    const workspace = await Workspace.load(process.env.ROTOTO_EXAMPLES_BASIC);
    const resolution = await workspace.resolveVariable("premium-message", { user: { tier: "premium" } });
    if (resolution.valueKey !== "premium") {
      throw new Error("unexpected resolution " + JSON.stringify(resolution));
    }
  `;
  execFileSync("node", ["--input-type=module", "-e", script], {
    cwd: temp,
    env: {
      ...process.env,
      ROTOTO_EXAMPLES_BASIC: resolve(root, "examples/basic"),
    },
    stdio: "inherit",
  });
} finally {
  rmSync(temp, { recursive: true, force: true });
}
