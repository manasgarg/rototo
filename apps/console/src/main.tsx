import { StrictMode, Suspense, lazy } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Navigate, Route, Routes, useParams } from "react-router";

import "./styles.css";

import { LoadingScreen } from "@/components/loading-screen";
import { useMe } from "@/lib/me";
import { MeProvider } from "@/lib/me";
import { normalizeEditKind, normalizeSection } from "@/lib/route-normalizers";
import { ConsoleScreen } from "@/screens/console-screen";
import { LoginScreen } from "@/screens/login-screen";
import { NotFound } from "@/screens/not-found";

const DraftScreen = lazy(async () => {
  const screen = await import("@/screens/draft-screen");
  return { default: screen.DraftScreen };
});

const WorkspaceScreen = lazy(async () => {
  const screen = await import("@/screens/workspace-screen");
  return { default: screen.WorkspaceScreen };
});

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
  return <WorkspaceScreen path={decodeEntityPath(splat)} workspaceId={workspaceId} />;
}

function DraftRoute({ screen }: { screen?: "overview" | "changes" | "validate" | "publish" }) {
  const { workspaceId = "", draftId = "" } = useParams();
  return (
    <DraftScreen draftId={draftId} screen={screen ?? "overview"} workspaceId={workspaceId} />
  );
}

function DraftEditKindRoute() {
  const { workspaceId = "", draftId = "", kind = "" } = useParams();
  const editKind = normalizeEditKind(kind);
  if (!editKind) {
    return <NotFound />;
  }
  return (
    <DraftScreen draftId={draftId} kind={editKind} screen="edit" workspaceId={workspaceId} />
  );
}

function DraftEntityRoute() {
  const { workspaceId = "", draftId = "", "*": splat = "" } = useParams();
  return (
    <DraftScreen
      draftId={draftId}
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
                  <Route element={<ConsoleScreen screen="repositories" />} path="/" />
                  <Route element={<ConsoleScreen screen="activity" />} path="activity" />
                  <Route element={<ConsoleScreen screen="drafts" />} path="drafts" />
                  <Route element={<ConsoleScreen screen="workspaces" />} path="workspaces" />
                  <Route element={<WorkspaceScreenRoute />} path="workspaces/:workspaceId" />
                  <Route
                    element={<WorkspaceEntityRoute />}
                    path="workspaces/:workspaceId/tree/*"
                  />
                  <Route
                    element={<DraftRoute />}
                    path="workspaces/:workspaceId/drafts/:draftId"
                  />
                  <Route
                    element={<DraftRoute screen="changes" />}
                    path="workspaces/:workspaceId/drafts/:draftId/changes"
                  />
                  <Route
                    element={<DraftRoute screen="validate" />}
                    path="workspaces/:workspaceId/drafts/:draftId/validate"
                  />
                  <Route
                    element={<DraftRoute screen="publish" />}
                    path="workspaces/:workspaceId/drafts/:draftId/publish"
                  />
                  <Route
                    element={<DraftEditKindRoute />}
                    path="workspaces/:workspaceId/drafts/:draftId/edit/:kind"
                  />
                  <Route
                    element={<DraftEntityRoute />}
                    path="workspaces/:workspaceId/drafts/:draftId/tree/*"
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
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </StrictMode>,
);
