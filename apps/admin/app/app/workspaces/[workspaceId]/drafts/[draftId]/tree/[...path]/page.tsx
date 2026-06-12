import { DraftScreen } from "../../draft-screen";

export const runtime = "nodejs";

export default async function DraftEntityPage({
  params,
}: {
  params: Promise<{ workspaceId: string; draftId: string; path: string[] }>;
}) {
  const { workspaceId, draftId, path } = await params;
  const entityPath = path.map(decodeURIComponent).join("/");
  return (
    <DraftScreen draftId={draftId} path={entityPath} screen="edit" workspaceId={workspaceId} />
  );
}
