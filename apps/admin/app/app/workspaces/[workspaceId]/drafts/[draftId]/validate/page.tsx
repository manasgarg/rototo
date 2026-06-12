import { DraftScreen } from "../draft-screen";

export const runtime = "nodejs";

export default async function DraftValidatePage({
  params,
}: {
  params: Promise<{ workspaceId: string; draftId: string }>;
}) {
  const { workspaceId, draftId } = await params;
  return <DraftScreen draftId={draftId} screen="validate" workspaceId={workspaceId} />;
}
