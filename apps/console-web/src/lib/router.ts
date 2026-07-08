// The console's information hierarchy, owned by one module. The hash path
// encodes containment (tree -> package -> entity): what you are looking at.
// Query params carry view state (change set, pin, chosen context): how you
// are looking at it. Package tails reuse the addressing grammar
// (design/addressing.md) instead of inventing a second URL grammar: a "-"
// segment ends the package path (ids and package paths both contain "/"),
// and the tail after it is either a page noun (surfaces, files, history) or
// an address (variable=checkout_redesign, catalog=plans:entry=pro). An
// address always contains "=" and a page noun never does, so parsing stays
// lexical.
//
//   #/                                     home
//   #/admin                                deployment admin
//   #/trees/st_7                           tree home
//   #/trees/st_7/changes                   change sets of the tree
//   #/trees/st_7/changes/cs_42             one change set
//   #/trees/st_7/examples/billing/-        package overview ("." packages
//                                          have no path segments: /trees/st_7/-)
//   #/trees/st_7/examples/billing/-/variable=active_plan
//   #/trees/st_7/examples/billing/-/catalog=plans:entry=pro
//   #/trees/st_7/examples/billing/-/surfaces/pricing
//   #/trees/st_7/examples/billing/-/files/variables/active_plan.toml
//   #/trees/st_7/examples/billing/-/history
//   ...?cs=cs_42&pin=<sha>&ctx=sample:premium

import { useEffect, useState } from "react";

export type AddressStep = { class: string; id: string };

export type PackageView =
    | { kind: "overview" }
    | { kind: "address"; steps: AddressStep[] }
    | { kind: "surfaces"; surfaceId: string | null }
    | { kind: "files"; file: string | null }
    | { kind: "history" };

export type Route =
    | { page: "home" }
    | { page: "tree"; treeId: string }
    | { page: "changes"; treeId: string }
    | { page: "change-set"; treeId: string; changeSetId: string }
    | {
          page: "package";
          treeId: string;
          packagePath: string;
          view: PackageView;
      }
    | { page: "admin" }
    | { page: "not-enrolled" };

// View state parameterizes every page rather than naming a place, so it
// rides in the query: `cs` is the change set edits accumulate on, `pin` is
// a read-only historical instant, `ctx` is the chosen context of the
// execution facet ("sample:<key>" or "synthetic:<label>"; ad-hoc JSON stays
// session state and deliberately does not survive a shared link).
export type ViewState = {
    changeSetId: string | null;
    pin: string | null;
    context: string | null;
};

export const EMPTY_STATE: ViewState = {
    changeSetId: null,
    pin: null,
    context: null,
};

// Human labels for address classes, used by breadcrumbs and collection
// headers so the two cannot drift.
export const CLASS_LABELS: Record<string, string> = {
    variable: "Variables",
    catalog: "Catalogs",
    entry: "Entries",
    list: "Lists",
    "evaluation-context": "Contexts",
    sample: "Samples",
    layer: "Layers",
    linter: "Linters",
    manifest: "Manifest",
    governance: "Governance",
};

export function parseHash(raw: string): { route: Route; state: ViewState } {
    const queryAt = raw.indexOf("?");
    const path = queryAt === -1 ? raw : raw.slice(0, queryAt);
    const query = new URLSearchParams(
        queryAt === -1 ? "" : raw.slice(queryAt + 1),
    );
    return {
        route: parsePath(path),
        state: {
            changeSetId: query.get("cs"),
            pin: query.get("pin"),
            context: query.get("ctx"),
        },
    };
}

function parsePath(path: string): Route {
    const segments = path.split("/").filter((segment) => segment !== "");
    if (segments.length === 0) {
        return { page: "home" };
    }
    if (segments[0] === "admin") {
        return { page: "admin" };
    }
    if (segments[0] === "not-enrolled") {
        return { page: "not-enrolled" };
    }
    if (segments[0] !== "trees" || segments[1] === undefined) {
        return { page: "home" };
    }
    const treeId = segments[1];
    const rest = segments.slice(2);
    const divider = rest.indexOf("-");
    if (divider !== -1) {
        const packagePath = rest.slice(0, divider).join("/") || ".";
        const tailSegments = rest.slice(divider + 1);
        // A namespace-subtree address ends in "/", which segment filtering
        // eats; restore it from the raw path.
        const tail =
            tailSegments.join("/") +
            (tailSegments.length > 0 && path.endsWith("/") ? "/" : "");
        return { page: "package", treeId, packagePath, view: parseTail(tail) };
    }
    if (rest.length === 0) {
        return { page: "tree", treeId };
    }
    if (rest[0] === "changes") {
        const changeSetId = rest.slice(1).join("/");
        return changeSetId === ""
            ? { page: "changes", treeId }
            : { page: "change-set", treeId, changeSetId };
    }
    return { page: "tree", treeId };
}

function parseTail(tail: string): PackageView {
    if (tail === "") {
        return { kind: "overview" };
    }
    const segments = tail.split("/");
    if (segments[0] === "surfaces") {
        const surfaceId = segments.slice(1).join("/");
        return {
            kind: "surfaces",
            surfaceId: surfaceId === "" ? null : surfaceId,
        };
    }
    if (segments[0] === "files") {
        const file = segments.slice(1).join("/");
        return { kind: "files", file: file === "" ? null : file };
    }
    if (tail === "history") {
        return { kind: "history" };
    }
    const steps = parseAddress(tail);
    return steps === null ? { kind: "overview" } : { kind: "address", steps };
}

// One step per ":", class before the first "=", id after it. JSON-pointer
// suffixes ("#/...") cannot ride in a URL fragment, so a pointer is dropped
// here; field-level focus stays in-page state.
export function parseAddress(text: string): AddressStep[] | null {
    const pointerAt = text.indexOf("#");
    const entityPath = pointerAt === -1 ? text : text.slice(0, pointerAt);
    if (entityPath === "") {
        return null;
    }
    const steps: AddressStep[] = [];
    for (const part of entityPath.split(":")) {
        const bind = part.indexOf("=");
        if (bind <= 0) {
            return null;
        }
        steps.push({ class: part.slice(0, bind), id: part.slice(bind + 1) });
    }
    return steps;
}

export function formatAddress(steps: AddressStep[]): string {
    return steps.map((step) => `${step.class}=${step.id}`).join(":");
}

// A collective ("variable=") or namespace subtree ("variable=payments/")
// selects many entities; anything else names one.
export function isCollective(step: AddressStep): boolean {
    return step.id === "" || step.id.endsWith("/");
}

export function homeUrl(): string {
    return "/";
}

export function adminUrl(): string {
    return "/admin";
}

export function treeUrl(treeId: string): string {
    return `/trees/${treeId}`;
}

export function changesUrl(treeId: string): string {
    return `/trees/${treeId}/changes`;
}

export function changeSetUrl(treeId: string, changeSetId: string): string {
    return `/trees/${treeId}/changes/${changeSetId}`;
}

export function packageUrl(
    treeId: string,
    packagePath: string,
    view: PackageView,
    state?: ViewState,
): string {
    const packageSegments =
        packagePath === "." || packagePath === "" ? "" : `/${packagePath}`;
    const tail =
        view.kind === "overview"
            ? ""
            : view.kind === "address"
              ? `/${formatAddress(view.steps)}`
              : view.kind === "surfaces"
                ? view.surfaceId === null
                    ? "/surfaces"
                    : `/surfaces/${view.surfaceId}`
                : view.kind === "files"
                  ? view.file === null
                      ? "/files"
                      : `/files/${view.file}`
                  : "/history";
    return `/trees/${treeId}${packageSegments}/-${tail}${formatState(state)}`;
}

function formatState(state?: ViewState): string {
    if (state === undefined) {
        return "";
    }
    const params = new URLSearchParams();
    if (state.changeSetId !== null) {
        params.set("cs", state.changeSetId);
    }
    if (state.pin !== null) {
        params.set("pin", state.pin);
    }
    if (state.context !== null) {
        params.set("ctx", state.context);
    }
    const text = params.toString();
    return text === "" ? "" : `?${text}`;
}

export function useHashPath(): string {
    const [hash, setHash] = useState(current);
    useEffect(() => {
        const onChange = () => setHash(current());
        window.addEventListener("hashchange", onChange);
        return () => window.removeEventListener("hashchange", onChange);
    }, []);
    return hash;
}

export function navigate(to: string): void {
    window.location.hash = to;
}

// Replace the current history entry instead of pushing one; for automatic
// forwards (a single-package tree lands on its package) where Back must
// not bounce.
export function redirect(to: string): void {
    window.location.replace(`#${to}`);
}

function current(): string {
    const hash = window.location.hash.replace(/^#/, "");
    return hash === "" ? "/" : hash;
}
