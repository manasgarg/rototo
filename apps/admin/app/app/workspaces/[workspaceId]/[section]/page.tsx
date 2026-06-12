import { notFound } from "next/navigation";
import { normalizeSection, WorkspaceScreen } from "../workspace-screen";

export const runtime = "nodejs";

export default async function WorkspaceSectionPage({
  params,
}: {
  params: Promise<{ workspaceId: string; section: string }>;
}) {
  const { workspaceId, section } = await params;
  const sectionId = normalizeSection(section);
  if (!sectionId || sectionId === "overview") {
    notFound();
  }
  return <WorkspaceScreen section={sectionId} workspaceId={workspaceId} />;
}
