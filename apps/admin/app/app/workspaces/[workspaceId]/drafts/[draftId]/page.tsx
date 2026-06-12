import { redirect } from "next/navigation";
import { encodeEntityPath } from "../../workspace-screen";
import { DraftScreen, normalizeDraftScreen, normalizeEditKind } from "./draft-screen";

export const runtime = "nodejs";

type SearchParams = {
  screen?: string | string[];
  kind?: string | string[];
  path?: string | string[];
};

export default async function DraftPage({
  params,
  searchParams,
}: {
  params: Promise<{ workspaceId: string; draftId: string }>;
  searchParams: Promise<SearchParams>;
}) {
  const [{ workspaceId, draftId }, query] = await Promise.all([params, searchParams]);
  // Legacy query URLs redirect to their path-based equivalents.
  const base = `/app/workspaces/${workspaceId}/drafts/${draftId}`;
  const path = singleParam(query.path);
  if (path) {
    redirect(`${base}/tree/${encodeEntityPath(path)}`);
  }
  const screen = normalizeDraftScreen(singleParam(query.screen));
  if (screen === "edit") {
    redirect(`${base}/edit/${normalizeEditKind(singleParam(query.kind)) ?? "variables"}`);
  }
  if (screen && screen !== "overview") {
    redirect(`${base}/${screen}`);
  }
  return <DraftScreen draftId={draftId} workspaceId={workspaceId} />;
}

function singleParam(value: string | string[] | undefined): string | null {
  if (Array.isArray(value)) {
    return value[0] ?? null;
  }
  return value ?? null;
}
