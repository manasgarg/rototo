import { WorkspaceScreen } from "../../workspace-screen";

export const runtime = "nodejs";

export default async function WorkspaceEntityPage({
  params,
}: {
  params: Promise<{ workspaceId: string; path: string[] }>;
}) {
  const { workspaceId, path } = await params;
  const entityPath = path.map(decodeURIComponent).join("/");
  return <WorkspaceScreen path={entityPath} workspaceId={workspaceId} />;
}
