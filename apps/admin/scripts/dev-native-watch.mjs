/* Runs `next dev` and restarts it whenever the rototo native module is
   rebuilt. Node cannot unload a loaded .node addon (N-API has no safe
   dlclose), so Fast Refresh can never pick up a rebuilt binary — a process
   restart is the only reload. This makes that restart automatic. */

import { spawn } from "node:child_process";
import { unwatchFile, watchFile } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const nativeModule = resolve(appDir, "../../sdks/typescript/rototo.linux-x64-gnu.node");
const passthroughArgs = process.argv.slice(2);

let child = null;
let restarting = false;
let shuttingDown = false;

function start() {
  child = spawn("npx", ["next", "dev", "--webpack", ...passthroughArgs], {
    cwd: appDir,
    stdio: "inherit",
  });
  child.on("exit", (code) => {
    if (shuttingDown) {
      process.exit(code ?? 0);
    }
    if (restarting) {
      restarting = false;
      start();
      return;
    }
    process.exit(code ?? 0);
  });
}

function restart(reason) {
  if (restarting || shuttingDown || child === null) {
    return;
  }
  restarting = true;
  console.log(`\n[dev-native-watch] ${reason} — restarting next dev to reload it\n`);
  child.kill("SIGTERM");
}

// watchFile (stat polling) survives the rebuild replacing the file, which
// fs.watch does not.
watchFile(nativeModule, { interval: 1500 }, (current, previous) => {
  if (current.mtimeMs !== previous.mtimeMs) {
    restart("rototo native module changed");
  }
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    shuttingDown = true;
    unwatchFile(nativeModule);
    child?.kill(signal);
  });
}

start();
