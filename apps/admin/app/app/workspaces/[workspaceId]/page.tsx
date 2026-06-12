import { redirect } from "next/navigation";
import { encodeEntityPath, normalizeSection, WorkspaceScreen } from "./workspace-screen";

export const runtime = "nodejs";

type SearchParams = {
  section?: string | string[];
  path?: string | string[];
};

export default async function WorkspacePage({
  params,
  searchParams,
}: {
  params: Promise<{ workspaceId: string }>;
  searchParams: Promise<SearchParams>;
}) {
  const [{ workspaceId }, query] = await Promise.all([params, searchParams]);
  // Legacy query URLs redirect to their path-based equivalents.
  const path = singleParam(query.path);
  if (path) {
    redirect(`/app/workspaces/${workspaceId}/tree/${encodeEntityPath(path)}`);
  }
  const section = normalizeSection(singleParam(query.section));
  if (section && section !== "overview") {
    redirect(`/app/workspaces/${workspaceId}/${section}`);
  }
  return <WorkspaceScreen workspaceId={workspaceId} />;
}

function singleParam(value: string | string[] | undefined): string | null {
  if (Array.isArray(value)) {
    return value[0] ?? null;
  }
  return value ?? null;
}
