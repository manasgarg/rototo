import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const sdk = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(sdk, "../..");
const temp = mkdtempSync(join(tmpdir(), "rototo-typescript-package-"));
const npmEnv = { ...process.env };
delete npmEnv.npm_config_prefix;
delete npmEnv.npm_config_local_prefix;
delete npmEnv.npm_config_global_prefix;
delete npmEnv.npm_config_globalconfig;
npmEnv.npm_config_cache = join(temp, ".npm-cache");

try {
    execFileSync("npm", ["pack", "--pack-destination", temp], {
        cwd: sdk,
        env: npmEnv,
        stdio: ["ignore", "ignore", "inherit"],
    });
    const tarballName = readdirSync(temp).find((file) => file.endsWith(".tgz"));
    if (!tarballName) {
        throw new Error("npm pack did not produce a tarball");
    }
    const tarball = join(temp, tarballName);

    execFileSync("npm", ["init", "-y"], {
        cwd: temp,
        env: npmEnv,
        stdio: "ignore",
    });
    execFileSync(
        "npm",
        [
            "install",
            "--ignore-scripts",
            "--omit=dev",
            "--omit=optional",
            "--omit=peer",
            "--package-lock=false",
            "--save=false",
            "--no-audit",
            "--no-fund",
            tarball,
        ],
        {
            cwd: temp,
            env: npmEnv,
            stdio: "inherit",
        },
    );

    const script = `
    import { Package, __version__ } from "rototo";
    if (!/^\\d+\\.\\d+\\.\\d+(-[0-9A-Za-z.-]+)?$/.test(__version__)) {
      throw new Error("unexpected version " + __version__);
    }
    const pkg = await Package.load(process.env.ROTOTO_EXAMPLES_BASIC);
    const resolution = await pkg.resolveVariable("premium-message", { user: { tier: "premium" } });
    if (resolution.value !== "Welcome back, premium member." || resolution.source.kind !== "literal") {
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
