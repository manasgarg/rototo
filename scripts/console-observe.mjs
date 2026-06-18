#!/usr/bin/env node
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const rawArgs = process.argv.slice(2);
const dirArgIndex = rawArgs.indexOf("--dir");
const dir =
  dirArgIndex >= 0 && rawArgs[dirArgIndex + 1]
    ? rawArgs[dirArgIndex + 1]
    : ".rototo/dev/observability";
const fullDir = join(root, dir);
const args = new Set(rawArgs);
const check = args.has("--check");
const watch = args.has("--watch");
let thresholds = defaultThresholds();

async function main() {
  if (watch) {
    await summarizeAndMaybeExit(false);
    setInterval(() => {
      void summarizeAndMaybeExit(false);
    }, 2000);
    return;
  }
  await summarizeAndMaybeExit(check);
}

async function summarizeAndMaybeExit(shouldCheck) {
  thresholds = await readThresholds(fullDir);
  const apiEvents = await readNdjson(join(fullDir, "console-api.ndjson"));
  const uiEvents = await readNdjson(join(fullDir, "console-ui.ndjson"));
  const summary = summarize(apiEvents, uiEvents);
  await mkdir(fullDir, { recursive: true });
  await writeFile(
    join(fullDir, "console-observe-summary.json"),
    `${JSON.stringify(summary, null, 2)}\n`,
  );
  printSummary(summary);

  if (shouldCheck && summary.actionable.length > 0) {
    process.exitCode = 1;
  }
}

async function readNdjson(path) {
  if (!existsSync(path)) {
    return [];
  }
  const text = await readFile(path, "utf8");
  const events = [];
  for (const [index, line] of text.split(/\r?\n/).entries()) {
    if (!line.trim()) {
      continue;
    }
    try {
      events.push(JSON.parse(line));
    } catch (error) {
      events.push({
        kind: "parse-error",
        path,
        line: index + 1,
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }
  return events;
}

async function readThresholds(dir) {
  const configPath = join(dir, "console-observability.json");
  if (!existsSync(configPath)) {
    return defaultThresholds();
  }
  try {
    const config = JSON.parse(await readFile(configPath, "utf8"));
    return {
      apiP95Ms: numberOr(config?.thresholds?.api_p95_ms, 750),
      apiErrorCount: numberOr(config?.thresholds?.api_errors, 0),
      frontendErrorCount: numberOr(config?.thresholds?.frontend_errors, 0),
      lspFailureCount: numberOr(config?.thresholds?.lsp_failures, 0),
    };
  } catch {
    return defaultThresholds();
  }
}

function defaultThresholds() {
  return {
    apiP95Ms: 750,
    apiErrorCount: 0,
    frontendErrorCount: 0,
    lspFailureCount: 0,
  };
}

function numberOr(value, fallback) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function summarize(apiEvents, uiEvents) {
  const routes = new Map();
  const backendErrors = [];
  const frontendErrors = [];
  const lspFailures = [];

  for (const event of apiEvents) {
    if (event.kind === "api-request") {
      const key = `${event.method || "GET"} ${event.route || event.path || "unknown"}`;
      const route = routes.get(key) || { route: key, latencies: [], statuses: new Map() };
      if (typeof event.latency_ms === "number") {
        route.latencies.push(event.latency_ms);
      }
      const status = String(event.status || "unknown");
      route.statuses.set(status, (route.statuses.get(status) || 0) + 1);
      routes.set(key, route);
      if (Number(event.status) >= 500) {
        backendErrors.push(event);
      }
    } else if (event.kind === "operation" && /lsp/i.test(event.operation || "") && event.ok === false) {
      lspFailures.push(event);
    } else if (event.kind === "parse-error") {
      backendErrors.push(event);
    }
  }

  for (const event of uiEvents) {
    if (
      event.kind === "frontend-error" ||
      event.kind === "unhandled-rejection" ||
      (event.kind === "api-fetch" && event.ok === false) ||
      event.kind === "parse-error"
    ) {
      frontendErrors.push(event);
    }
  }

  const routeSummaries = Array.from(routes.values()).map((route) => {
    route.latencies.sort((left, right) => left - right);
    const statuses = Object.fromEntries(route.statuses.entries());
    return {
      route: route.route,
      count: route.latencies.length,
      p50_ms: percentile(route.latencies, 0.50),
      p95_ms: percentile(route.latencies, 0.95),
      p99_ms: percentile(route.latencies, 0.99),
      max_ms: route.latencies.at(-1) ?? null,
      statuses,
    };
  }).sort((left, right) => (right.p95_ms || 0) - (left.p95_ms || 0));

  const actionable = [];
  for (const route of routeSummaries) {
    if ((route.p95_ms || 0) > thresholds.apiP95Ms) {
      actionable.push({
        severity: "warning",
        kind: "slow-api-route",
        message: `${route.route} p95=${route.p95_ms}ms exceeds ${thresholds.apiP95Ms}ms`,
      });
    }
  }
  if (backendErrors.length > thresholds.apiErrorCount) {
    actionable.push({
      severity: "error",
      kind: "backend-errors",
      message: `${backendErrors.length} backend error event(s) observed`,
    });
  }
  if (frontendErrors.length > thresholds.frontendErrorCount) {
    actionable.push({
      severity: "error",
      kind: "frontend-errors",
      message: `${frontendErrors.length} frontend error event(s) observed`,
    });
  }
  if (lspFailures.length > thresholds.lspFailureCount) {
    actionable.push({
      severity: "error",
      kind: "lsp-failures",
      message: `${lspFailures.length} LSP failure event(s) observed`,
    });
  }

  return {
    generatedAt: new Date().toISOString(),
    directory: dir,
    thresholds,
    totals: {
      apiEvents: apiEvents.length,
      uiEvents: uiEvents.length,
      backendErrors: backendErrors.length,
      frontendErrors: frontendErrors.length,
      lspFailures: lspFailures.length,
    },
    slowestRoutes: routeSummaries.slice(0, 10),
    backendErrors: backendErrors.slice(-20),
    frontendErrors: frontendErrors.slice(-20),
    lspFailures: lspFailures.slice(-20),
    actionable,
  };
}

function printSummary(summary) {
  console.log(`console observability: ${summary.directory}`);
  console.log(
    `events api=${summary.totals.apiEvents} ui=${summary.totals.uiEvents} backendErrors=${summary.totals.backendErrors} frontendErrors=${summary.totals.frontendErrors} lspFailures=${summary.totals.lspFailures}`,
  );
  for (const route of summary.slowestRoutes.slice(0, 5)) {
    console.log(
      `route ${route.route} count=${route.count} p50=${route.p50_ms ?? "-"}ms p95=${route.p95_ms ?? "-"}ms p99=${route.p99_ms ?? "-"}ms max=${route.max_ms ?? "-"}ms`,
    );
  }
  if (summary.actionable.length === 0) {
    console.log("no actionable findings above thresholds");
  } else {
    console.log("actionable findings:");
    for (const finding of summary.actionable) {
      console.log(`- ${finding.severity} ${finding.kind}: ${finding.message}`);
    }
  }
}

function percentile(values, p) {
  if (values.length === 0) {
    return null;
  }
  const index = Math.min(values.length - 1, Math.ceil(values.length * p) - 1);
  return Math.round(values[index]);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack || error.message : String(error));
  process.exit(1);
});
