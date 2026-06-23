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

const PackageScreen = lazyWithRetry(
    async () => {
        const screen = await import("@/screens/package-screen");
        return { default: screen.PackageScreen };
    },
    async () => {
        const screen = (await import(
            /* @vite-ignore */ `/src/screens/package-screen.tsx?t=${Date.now()}`
        )) as typeof import("@/screens/package-screen");
        return { default: screen.PackageScreen };
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

function PackageSectionRoute() {
    const { packageId = "", section = "" } = useParams();
    const sectionId = normalizeSection(section);
    if (!sectionId || sectionId === "overview") {
        return <NotFound />;
    }
    return <PackageScreen section={sectionId} packageId={packageId} />;
}

function PackageEntityRoute() {
    const { packageId = "", "*": splat = "" } = useParams();
    return (
        <PackageScreen path={decodeEntityPath(splat)} packageId={packageId} />
    );
}

function BranchRoute({
    screen,
}: {
    screen?: "overview" | "changes" | "validate" | "publish";
}) {
    const { packageId = "", branchId = "" } = useParams();
    return (
        <BranchScreen
            branchId={branchId}
            screen={screen ?? "overview"}
            packageId={packageId}
        />
    );
}

function BranchEditKindRoute() {
    const { packageId = "", branchId = "", kind = "" } = useParams();
    const editKind = normalizeEditKind(kind);
    if (!editKind) {
        return <NotFound />;
    }
    return (
        <BranchScreen
            branchId={branchId}
            kind={editKind}
            screen="edit"
            packageId={packageId}
        />
    );
}

function BranchEntityRoute() {
    const { packageId = "", branchId = "", "*": splat = "" } = useParams();
    return (
        <BranchScreen
            branchId={branchId}
            path={decodeEntityPath(splat)}
            screen="edit"
            packageId={packageId}
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
                                            <ConsoleScreen screen="packages" />
                                        }
                                        path="packages"
                                    />
                                    <Route
                                        element={<PackageScreenRoute />}
                                        path="packages/:packageId"
                                    />
                                    <Route
                                        element={<PackageEntityRoute />}
                                        path="packages/:packageId/tree/*"
                                    />
                                    <Route
                                        element={<BranchRoute />}
                                        path="packages/:packageId/branches/:branchId"
                                    />
                                    <Route
                                        element={
                                            <BranchRoute screen="changes" />
                                        }
                                        path="packages/:packageId/branches/:branchId/changes"
                                    />
                                    <Route
                                        element={
                                            <BranchRoute screen="validate" />
                                        }
                                        path="packages/:packageId/branches/:branchId/validate"
                                    />
                                    <Route
                                        element={
                                            <BranchRoute screen="publish" />
                                        }
                                        path="packages/:packageId/branches/:branchId/publish"
                                    />
                                    <Route
                                        element={<BranchEditKindRoute />}
                                        path="packages/:packageId/branches/:branchId/edit/:kind"
                                    />
                                    <Route
                                        element={<BranchEntityRoute />}
                                        path="packages/:packageId/branches/:branchId/tree/*"
                                    />
                                    <Route
                                        element={<PackageSectionRoute />}
                                        path="packages/:packageId/:section"
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

function PackageScreenRoute() {
    const { packageId = "" } = useParams();
    return <PackageScreen packageId={packageId} />;
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
