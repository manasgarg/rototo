import { ConsoleScreen } from "../console-screen";

export default async function WorkspacesPage({
  searchParams,
}: {
  searchParams: Promise<{ repo?: string | string[] }>;
}) {
  const query = await searchParams;
  const repo = Array.isArray(query.repo) ? query.repo[0] ?? null : query.repo ?? null;
  return <ConsoleScreen repoId={repo} screen="workspaces" />;
}
