import { WorkspaceScreen } from "../workspace-screen";

export const runtime = "nodejs";

export default async function WorkspaceDraftsPage({
  params,
}: {
  params: Promise<{ workspaceId: string }>;
}) {
  const { workspaceId } = await params;
  return <WorkspaceScreen section="drafts" workspaceId={workspaceId} />;
}
