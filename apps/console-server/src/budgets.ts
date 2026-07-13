// The latency budgets from the console implementation plan, enforced in CI
// from C1 onward. These are p95 budgets over warm paths: a budget miss is a
// gate failure, not a log line.

export const BUDGETS_MS = {
    // Any UI-blocking API answer (browsing, capability rendering).
    interaction: 100,
    // An edit acknowledged end to end. Measured once the C2 save path
    // exists; the constant is wired now so the harness never forgets it.
    saveAck: 300,
    // A resolution preview against an already-staged (cached) pin.
    preview: 500,
} as const;
