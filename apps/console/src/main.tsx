import {
    Component,
    StrictMode,
    Suspense,
    lazy,
    type ComponentType,
    type ErrorInfo,
    type ReactNode,
} from "react";
import { createRoot } from "react-dom/client";
import {
    BrowserRouter,
    Navigate,
    Route,
    Routes,
    useParams,
} from "react-router";

import "./styles.css";

import { LoadingScreen } from "@/components/loading-screen";
import { useMe } from "@/lib/me";
import { MeProvider } from "@/lib/me";
import {
    describeError,
    installGlobalErrorTelemetry,
    recordConsoleEvent,
} from "@/lib/observability";
import { normalizeEditKind, normalizeSection } from "@/lib/route-normalizers";
import { ConsoleScreen } from "@/screens/console-screen";
import { LoginScreen } from "@/screens/login-screen";
import { NotFound } from "@/screens/not-found";

const DYNAMIC_IMPORT_RETRY_DELAYS_MS = [150, 500, 1200];

const BranchScreen = lazyWithRetry(
    async () => {
        const screen = await import("@/screens/branch-screen");
        return { default: screen.BranchScreen };
    },
    async () => {
        const screen = (await import(
            /* @vite-ignore */ `/src/screens/branch-screen.tsx?t=${Date.now()}`
        )) as typeof import("@/screens/branch-screen");
        return { default: screen.BranchScreen };
    },
);

const WorkspaceScreen = lazyWithRetry(
    async () => {
        const screen = await import("@/screens/workspace-screen");
        return { default: screen.WorkspaceScreen };
    },
    async () => {
        const screen = (await import(
            /* @vite-ignore */ `/src/screens/workspace-screen.tsx?t=${Date.now()}`
        )) as typeof import("@/screens/workspace-screen");
        return { default: screen.WorkspaceScreen };
    },
);

installGlobalErrorTelemetry();

class TelemetryBoundary extends Component<
    { children: ReactNode },
    { failed: boolean; message: string | null }
> {
    state = { failed: false, message: null };

    static getDerivedStateFromError(error: unknown) {
        return { failed: true, message: errorMessage(error) };
    }

    componentDidCatch(error: Error, errorInfo: ErrorInfo) {
        recordConsoleEvent({
            kind: "frontend-error",
            error: describeError(error),
            componentStack: errorInfo.componentStack,
        });
    }

    render() {
        if (this.state.failed) {
            return (
                <main className="fault-page">
                    <div className="fault-panel">
                        <span className="label">console error</span>
                        <h1>The console UI failed to render.</h1>
                        <p className="hint">
                            {this.state.message ??
                                "Check the dev observability log for details."}
                        </p>
                        <button
                            className="btn btn-secondary"
                            onClick={() => window.location.reload()}
                            type="button"
                        >
                            Reload console
                        </button>
                    </div>
                </main>
            );
        }
        return this.props.children;
    }
}

function errorMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
}

function lazyWithRetry<T extends ComponentType<any>>(
    loader: () => Promise<{ default: T }>,
    devLoader: () => Promise<{ default: T }>,
) {
    return lazy(() => retryDynamicImport(loader, devLoader));
}

async function retryDynamicImport<T>(
    loader: () => Promise<T>,
    devLoader: () => Promise<T>,
): Promise<T> {
    let lastError: unknown;
    for (const delayMs of [0, ...DYNAMIC_IMPORT_RETRY_DELAYS_MS]) {
        if (delayMs > 0) {
            await new Promise((resolve) => setTimeout(resolve, delayMs));
        }
        try {
            const load =
                delayMs > 0 && import.meta.env.DEV ? devLoader : loader;
            return await load();
        } catch (error) {
            lastError = error;
            if (!isRetryableDynamicImportError(error)) {
                throw error;
            }
            recordConsoleEvent({
                kind: "dynamic-import-retry",
                error: describeError(error),
                delayMs,
            });
        }
    }
    throw lastError;
}

function isRetryableDynamicImportError(error: unknown): boolean {
    const message = errorMessage(error);
    return (
        message.includes("Failed to fetch dynamically imported module") ||
        message.includes("error loading dynamically imported module") ||
        message.includes("Importing a module script failed") ||
        message.includes("Loading chunk") ||
        message.includes("dynamically imported module")
    );
}

function RequireAuth({ children }: { children: React.ReactNode }) {
    const { me, error, loading } = useMe();
    if (loading) {
        return <LoadingScreen />;
    }
    if (error) {
        return (
            <main className="fault-page">
                <div className="fault-panel">
                    <span className="label">console unreachable</span>
                    <h1>The console API did not respond.</h1>
                    <p className="hint">{error}</p>
                </div>
            </main>
        );
    }
    if (!me?.user) {
        return <Navigate replace to="/login" />;
    }
    return <>{children}</>;
}

function WorkspaceSectionRoute() {
    const { workspaceId = "", section = "" } = useParams();
    const sectionId = normalizeSection(section);
    if (!sectionId || sectionId === "overview") {
        return <NotFound />;
    }
    return <WorkspaceScreen section={sectionId} workspaceId={workspaceId} />;
}

function WorkspaceEntityRoute() {
    const { workspaceId = "", "*": splat = "" } = useParams();
    return (
        <WorkspaceScreen
            path={decodeEntityPath(splat)}
            workspaceId={workspaceId}
        />
    );
}

function BranchRoute({
    screen,
}: {
    screen?: "overview" | "changes" | "validate" | "publish";
}) {
    const { workspaceId = "", branchId = "" } = useParams();
    return (
        <BranchScreen
            branchId={branchId}
            screen={screen ?? "overview"}
            workspaceId={workspaceId}
        />
    );
}

function BranchEditKindRoute() {
    const { workspaceId = "", branchId = "", kind = "" } = useParams();
    const editKind = normalizeEditKind(kind);
    if (!editKind) {
        return <NotFound />;
    }
    return (
        <BranchScreen
            branchId={branchId}
            kind={editKind}
            screen="edit"
            workspaceId={workspaceId}
        />
    );
}

function BranchEntityRoute() {
    const { workspaceId = "", branchId = "", "*": splat = "" } = useParams();
    return (
        <BranchScreen
            branchId={branchId}
            path={decodeEntityPath(splat)}
            screen="edit"
            workspaceId={workspaceId}
        />
    );
}

function decodeEntityPath(splat: string): string {
    return splat.split("/").map(decodeURIComponent).join("/");
}

function App() {
    return (
        <MeProvider>
            <Suspense fallback={<LoadingScreen />}>
                <Routes>
                    <Route element={<Navigate replace to="/app" />} path="/" />
                    <Route element={<LoginScreen />} path="/login" />
                    <Route
                        element={
                            <RequireAuth>
                                <Routes>
                                    <Route
                                        element={
                                            <Navigate
                                                replace
                                                to="configuration-sources"
                                            />
                                        }
                                        path="/"
                                    />
                                    <Route
                                        element={
                                            <ConsoleScreen screen="configuration-sources" />
                                        }
                                        path="configuration-sources"
                                    />
                                    <Route
                                        element={
                                            <Navigate
                                                replace
                                                to="/app/configuration-sources"
                                            />
                                        }
                                        path="source-trees"
                                    />
                                    <Route
                                        element={
                                            <ConsoleScreen screen="activity" />
                                        }
                                        path="activity"
                                    />
                                    <Route
                                        element={
                                            <ConsoleScreen screen="branches" />
                                        }
                                        path="branches"
                                    />
                                    <Route
                                        element={
                                            <ConsoleScreen screen="workspaces" />
                                        }
                                        path="workspaces"
                                    />
                                    <Route
                                        element={<WorkspaceScreenRoute />}
                                        path="workspaces/:workspaceId"
                                    />
                                    <Route
                                        element={<WorkspaceEntityRoute />}
                                        path="workspaces/:workspaceId/tree/*"
                                    />
                                    <Route
                                        element={<BranchRoute />}
                                        path="workspaces/:workspaceId/branches/:branchId"
                                    />
                                    <Route
                                        element={
                                            <BranchRoute screen="changes" />
                                        }
                                        path="workspaces/:workspaceId/branches/:branchId/changes"
                                    />
                                    <Route
                                        element={
                                            <BranchRoute screen="validate" />
                                        }
                                        path="workspaces/:workspaceId/branches/:branchId/validate"
                                    />
                                    <Route
                                        element={
                                            <BranchRoute screen="publish" />
                                        }
                                        path="workspaces/:workspaceId/branches/:branchId/publish"
                                    />
                                    <Route
                                        element={<BranchEditKindRoute />}
                                        path="workspaces/:workspaceId/branches/:branchId/edit/:kind"
                                    />
                                    <Route
                                        element={<BranchEntityRoute />}
                                        path="workspaces/:workspaceId/branches/:branchId/tree/*"
                                    />
                                    <Route
                                        element={<WorkspaceSectionRoute />}
                                        path="workspaces/:workspaceId/:section"
                                    />
                                    <Route element={<NotFound />} path="*" />
                                </Routes>
                            </RequireAuth>
                        }
                        path="/app/*"
                    />
                    <Route element={<NotFound />} path="*" />
                </Routes>
            </Suspense>
        </MeProvider>
    );
}

function WorkspaceScreenRoute() {
    const { workspaceId = "" } = useParams();
    return <WorkspaceScreen workspaceId={workspaceId} />;
}

createRoot(document.getElementById("root")!).render(
    <StrictMode>
        <TelemetryBoundary>
            <BrowserRouter>
                <App />
            </BrowserRouter>
        </TelemetryBoundary>
    </StrictMode>,
);
