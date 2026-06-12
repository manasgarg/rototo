import { redirect } from "next/navigation";
import { ConsoleScreen } from "./console-screen";

type SearchParams = {
  screen?: string | string[];
  repo?: string | string[];
};

export default async function AppPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  // Legacy query URLs redirect to their path-based equivalents.
  const query = await searchParams;
  const screen = singleParam(query.screen);
  if (screen === "workspaces" || screen === "drafts" || screen === "activity") {
    const repo = singleParam(query.repo);
    redirect(
      screen === "workspaces" && repo
        ? `/app/workspaces?repo=${encodeURIComponent(repo)}`
        : `/app/${screen}`,
    );
  }
  return <ConsoleScreen screen="repositories" />;
}

function singleParam(value: string | string[] | undefined): string | null {
  if (Array.isArray(value)) {
    return value[0] ?? null;
  }
  return value ?? null;
}
