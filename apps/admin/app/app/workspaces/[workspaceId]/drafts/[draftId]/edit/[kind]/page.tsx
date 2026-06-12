import { notFound } from "next/navigation";
import { DraftScreen, normalizeEditKind } from "../../draft-screen";

export const runtime = "nodejs";

export default async function DraftEditKindPage({
  params,
}: {
  params: Promise<{ workspaceId: string; draftId: string; kind: string }>;
}) {
  const { workspaceId, draftId, kind } = await params;
  const editKind = normalizeEditKind(kind);
  if (!editKind) {
    notFound();
  }
  return (
    <DraftScreen draftId={draftId} kind={editKind} screen="edit" workspaceId={workspaceId} />
  );
}
