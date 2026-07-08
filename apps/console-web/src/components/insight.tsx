// The remaining read-side panels (tranche C3): upcoming changes (the time
// facet), the package validity report, the composition tree, and package
// history with the "what was this value on March 3rd" bound.

import { useEffect, useState } from "react";

import {
    fetchComposition,
    fetchContexts,
    fetchFleet,
    fetchHistory,
    fetchUpcoming,
    runMatrix,
    type CommitRecord,
    type CompositionEdge,
    type FleetOverlayHealth,
    type LintDiagnostic,
    type MatrixColumn,
    type SampleContext,
    type UpcomingChange,
} from "@/lib/api";

// Behavior that changes with no commit and no deploy: env.now boundaries
// that have not passed. Empty means nothing is scheduled, and the panel
// says so only when asked (it hides when quiet).
export function UpcomingPanel({
    treeId,
    packagePath,
    pin,
}: {
    treeId: string;
    packagePath: string;
    pin: string;
}) {
    const [changes, setChanges] = useState<UpcomingChange[] | null>(null);
    useEffect(() => {
        let stale = false;
        fetchUpcoming(treeId, packagePath, pin).then(
            (response) => {
                if (!stale) {
                    setChanges(response.changes);
                }
            },
            () => {
                if (!stale) {
                    setChanges([]);
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, packagePath, pin]);

    if (changes === null || changes.length === 0) {
        return null;
    }
    return (
        <>
            <div className="section-header-text">
                <h2>Scheduled to change</h2>
                <p className="hint">
                    These flips happen on their own when the instant passes; no
                    commit, no deploy.
                </p>
            </div>
            <div className="row-list">
                {changes.map((change, index) => (
                    <div className="row row-static" key={index}>
                        <span className="row-text">
                            <span className="row-title mono">
                                {change.variable}
                            </span>
                            <span className="row-sub mono">
                                {siteLabel(change.site)}: {change.expression}
                            </span>
                        </span>
                        <span className="row-side">
                            <span className="pill pill-info mono">
                                {change.boundary}
                            </span>
                        </span>
                    </div>
                ))}
            </div>
        </>
    );
}

function siteLabel(site: UpcomingChange["site"]): string {
    switch (site.kind) {
        case "rule":
            return `rule ${site.index}`;
        case "queryFilter":
            return "query filter";
        case "querySort":
            return "query sort";
    }
}

// The validity facet at ring 1: every diagnostic lint reports, in one
// place, grouped by severity.
export function ValidityPanel({
    diagnostics,
}: {
    diagnostics: LintDiagnostic[];
}) {
    const [open, setOpen] = useState(false);
    const errors = diagnostics.filter(
        (diagnostic) => diagnostic.severity === "error",
    );
    const warnings = diagnostics.filter(
        (diagnostic) => diagnostic.severity !== "error",
    );
    return (
        <>
            <div className="section-header">
                <div className="section-header-text">
                    <h2>Health</h2>
                    <p className="hint">
                        {diagnostics.length === 0
                            ? "Lint is clean at this pin."
                            : `${errors.length} error${errors.length === 1 ? "" : "s"}, ${warnings.length} other finding${warnings.length === 1 ? "" : "s"}.`}
                    </p>
                </div>
                {diagnostics.length > 0 ? (
                    <button
                        className="btn btn-ghost btn-sm"
                        onClick={() => setOpen(!open)}
                    >
                        {open ? "Hide" : "Show"}
                    </button>
                ) : null}
            </div>
            {open ? (
                <div className="diagnostic-group">
                    {[...errors, ...warnings].map((diagnostic, index) => (
                        <p className="diagnostic" key={index}>
                            <span
                                className={`pill ${diagnostic.severity === "error" ? "pill-err" : "pill-warn"}`}
                            >
                                {diagnostic.severity}
                            </span>{" "}
                            <span className="mono">
                                {diagnostic.rule ?? ""}
                            </span>{" "}
                            {diagnostic.message}
                            {diagnostic.location?.path !== undefined ? (
                                <span className="hint">
                                    {" "}
                                    ({diagnostic.location.path})
                                </span>
                            ) : null}
                        </p>
                    ))}
                </div>
            ) : null}
        </>
    );
}

// Ring 2: the composition the extends edges imply. Hidden for a tree of
// independent packages; a base and its overlays draw as an indented tree.
export function CompositionPanel({
    treeId,
    refName,
    onOpenPackage,
}: {
    treeId: string;
    refName: string | undefined;
    onOpenPackage: (path: string) => void;
}) {
    const [edges, setEdges] = useState<CompositionEdge[] | null>(null);
    const [nodes, setNodes] = useState<string[]>([]);
    useEffect(() => {
        let stale = false;
        fetchComposition(treeId, refName).then(
            (response) => {
                if (!stale) {
                    setNodes(response.nodes.map((node) => node.path));
                    setEdges(response.edges);
                }
            },
            () => {
                if (!stale) {
                    setEdges([]);
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, refName]);

    if (edges === null || edges.length === 0) {
        return null;
    }
    // Bases first, their overlays indented beneath them.
    const overlaysOf = (base: string) =>
        edges.filter((edge) => edge.to === base).map((edge) => edge.from);
    const overlaySet = new Set(edges.map((edge) => edge.from));
    const roots = nodes.filter((node) => !overlaySet.has(node));

    return (
        <>
            <div className="section-header-text">
                <h2>Composition</h2>
                <p className="hint">
                    Inferred from each manifest's extends; nothing declares this
                    tree separately.
                </p>
            </div>
            <div className="row-list">
                {roots.map((root) => (
                    <div key={root}>
                        <button
                            className="row"
                            onClick={() => onOpenPackage(root)}
                        >
                            <span className="row-title mono">{root}</span>
                        </button>
                        {overlaysOf(root).map((overlay) => (
                            <button
                                className="row row-indent"
                                key={overlay}
                                onClick={() => onOpenPackage(overlay)}
                            >
                                <span className="row-text">
                                    <span className="row-title mono">
                                        {overlay}
                                    </span>
                                    <span className="row-sub">
                                        extends {root}
                                    </span>
                                </span>
                            </button>
                        ))}
                    </div>
                ))}
            </div>
        </>
    );
}

// Ring 2, both remaining facets (tranche C6). Validity: every overlay of
// this base composed and linted, aggregated — what makes evolving a base
// under tenants safe. Execution: one context resolved across the base and
// every overlay, as a matrix. Hidden entirely when the package has no
// overlays.
export function FleetPanel({
    treeId,
    packagePath,
    pin,
}: {
    treeId: string;
    packagePath: string;
    pin: string;
}) {
    const [overlays, setOverlays] = useState<FleetOverlayHealth[] | null>(null);
    const [samples, setSamples] = useState<SampleContext[]>([]);
    const [picked, setPicked] = useState<number>(-1);
    const [columns, setColumns] = useState<MatrixColumn[] | null>(null);

    useEffect(() => {
        let stale = false;
        setOverlays(null);
        setColumns(null);
        setPicked(-1);
        fetchFleet(treeId, packagePath, pin).then(
            (response) => {
                if (stale) {
                    return;
                }
                setOverlays(response.overlays);
                if (response.overlays.length > 0) {
                    fetchContexts(treeId, packagePath, pin).then(
                        (inventory) => {
                            if (!stale) {
                                setSamples(
                                    inventory.samples.filter(
                                        (sample) => sample.context !== null,
                                    ),
                                );
                            }
                        },
                        () => undefined,
                    );
                }
            },
            () => {
                if (!stale) {
                    setOverlays([]);
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, packagePath, pin]);

    useEffect(() => {
        const sample = samples[picked];
        if (sample?.context == null) {
            setColumns(null);
            return;
        }
        let stale = false;
        setColumns(null);
        runMatrix(treeId, packagePath, pin, sample.context).then(
            (response) => {
                if (!stale) {
                    setColumns(response.columns);
                }
            },
            () => undefined,
        );
        return () => {
            stale = true;
        };
    }, [treeId, packagePath, pin, samples, picked]);

    if (overlays === null || overlays.length === 0) {
        return null;
    }
    const failing = overlays.filter((overlay) => !overlay.ok).length;
    const variableIds = [
        ...new Set(
            (columns ?? []).flatMap((column) =>
                column.outcomes.map((outcome) => outcome.id),
            ),
        ),
    ].sort();

    return (
        <>
            <div className="section-header">
                <div className="section-header-text">
                    <h2>Fleet</h2>
                    <p className="hint">
                        {failing === 0
                            ? `All ${overlays.length} overlay${overlays.length === 1 ? "" : "s"} of this base lint clean.`
                            : `${failing} of ${overlays.length} overlay${overlays.length === 1 ? "" : "s"} fail lint against this base.`}
                    </p>
                </div>
                {samples.length > 0 ? (
                    <select
                        className="input"
                        value={picked}
                        onChange={(event) =>
                            setPicked(Number(event.target.value))
                        }
                    >
                        <option value={-1}>compare a context…</option>
                        {samples.map((sample, index) => (
                            <option key={index} value={index}>
                                {sample.evaluationContext}/{sample.key}
                            </option>
                        ))}
                    </select>
                ) : null}
            </div>
            <div className="row-list">
                {overlays.map((overlay) => (
                    <div className="row row-static" key={overlay.path}>
                        <span className="row-text">
                            <span className="row-title mono">
                                {overlay.path}
                            </span>
                            {overlay.failure !== undefined ? (
                                <span className="row-sub">
                                    {overlay.failure}
                                </span>
                            ) : null}
                        </span>
                        <span className="row-side">
                            {overlay.ok ? (
                                <span className="pill pill-ok">clean</span>
                            ) : (
                                <span className="pill pill-err">
                                    {overlay.errors} error
                                    {overlay.errors === 1 ? "" : "s"}
                                </span>
                            )}
                            {overlay.warnings > 0 ? (
                                <span className="pill pill-warn">
                                    {overlay.warnings} warning
                                    {overlay.warnings === 1 ? "" : "s"}
                                </span>
                            ) : null}
                        </span>
                    </div>
                ))}
            </div>
            {picked >= 0 ? (
                columns === null ? (
                    <p className="muted">Resolving across the fleet…</p>
                ) : (
                    <div className="table-scroll">
                        <table className="data-table">
                            <thead>
                                <tr>
                                    <th>variable</th>
                                    {columns.map((column) => (
                                        <th className="mono" key={column.path}>
                                            {column.path}
                                        </th>
                                    ))}
                                </tr>
                            </thead>
                            <tbody>
                                {variableIds.map((id) => (
                                    <tr key={id}>
                                        <td className="mono">{id}</td>
                                        {columns.map((column) => {
                                            const outcome =
                                                column.outcomes.find(
                                                    (entry) => entry.id === id,
                                                );
                                            return (
                                                <td
                                                    className="mono"
                                                    key={column.path}
                                                    title={
                                                        column.failure ??
                                                        outcome?.error ??
                                                        undefined
                                                    }
                                                >
                                                    {column.failure !==
                                                    undefined
                                                        ? "✗"
                                                        : outcome === undefined
                                                          ? "—"
                                                          : outcome.error !==
                                                              null
                                                            ? "!"
                                                            : clip(
                                                                  outcome.value,
                                                              )}
                                                </td>
                                            );
                                        })}
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    </div>
                )
            ) : null}
        </>
    );
}

function clip(value: unknown): string {
    const text = JSON.stringify(value);
    return text.length > 40 ? `${text.slice(0, 37)}…` : text;
}

// Package history, and the compliance question: pick an instant, get the
// pin that was in force, browse the package as it was.
export function HistoryPanel({
    treeId,
    packagePath,
    viewingPin,
    onViewPin,
}: {
    treeId: string;
    packagePath: string;
    viewingPin: string | null;
    onViewPin: (pin: string | null) => void;
}) {
    const [commits, setCommits] = useState<CommitRecord[] | null>(null);
    const [until, setUntil] = useState("");
    const [bound, setBound] = useState<string | undefined>(undefined);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        let stale = false;
        setCommits(null);
        fetchHistory(treeId, packagePath, bound).then(
            (response) => {
                if (!stale) {
                    setCommits(response.commits);
                }
            },
            (failure: Error) => {
                if (!stale) {
                    setError(failure.message);
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, packagePath, bound]);

    return (
        <div className="section">
            <div className="section-header-text">
                <h2>History</h2>
                <p className="hint">
                    Every commit that touched this package. Ask for an instant
                    to see what was in force then.
                </p>
            </div>
            <form
                className="field-row"
                onSubmit={(event) => {
                    event.preventDefault();
                    setBound(
                        until.trim() === ""
                            ? undefined
                            : normalizeInstant(until.trim()),
                    );
                }}
            >
                <span className="label">On this date</span>
                <input
                    className="input mono"
                    placeholder="2026-03-03 or 2026-03-03T12:00:00Z"
                    value={until}
                    onChange={(event) => setUntil(event.target.value)}
                />
                <button className="btn btn-secondary btn-sm" type="submit">
                    Look up
                </button>
                {bound !== undefined ? (
                    <button
                        className="btn btn-ghost btn-sm"
                        type="button"
                        onClick={() => {
                            setBound(undefined);
                            setUntil("");
                        }}
                    >
                        Clear
                    </button>
                ) : null}
            </form>
            {error !== null ? (
                <div className="banner banner-err">{error}</div>
            ) : null}
            {commits === null ? (
                <p className="muted">Loading…</p>
            ) : commits.length === 0 ? (
                <p className="hint">
                    No commits touched this package before that instant.
                </p>
            ) : (
                <div className="row-list">
                    {commits.map((commit, index) => {
                        const current = viewingPin === commit.sha;
                        const inForce = bound !== undefined && index === 0;
                        return (
                            <button
                                className="row"
                                data-active={current ? "true" : undefined}
                                key={commit.sha}
                                onClick={() =>
                                    onViewPin(current ? null : commit.sha)
                                }
                            >
                                <span className="row-text">
                                    <span className="row-title">
                                        {commit.message.split("\n")[0]}
                                    </span>
                                    <span className="row-sub mono">
                                        {commit.sha.slice(0, 10)} ·{" "}
                                        {commit.date}
                                        {commit.authorName !== null
                                            ? ` · ${commit.authorName}`
                                            : ""}
                                    </span>
                                </span>
                                <span className="row-side">
                                    {inForce ? (
                                        <span className="pill pill-ok">
                                            in force then
                                        </span>
                                    ) : null}
                                    {current ? (
                                        <span className="pill pill-info">
                                            viewing
                                        </span>
                                    ) : null}
                                </span>
                            </button>
                        );
                    })}
                </div>
            )}
        </div>
    );
}

// A date alone means "end of that day, UTC".
function normalizeInstant(value: string): string {
    return /^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T23:59:59Z` : value;
}
