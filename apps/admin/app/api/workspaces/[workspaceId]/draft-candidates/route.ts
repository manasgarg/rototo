import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import { getWorkspaceForUser, listDraftSessionsForWorkspace } from "@/lib/db";
import { compareGitHubRefs, githubErrorMessage, listGitHubBranches } from "@/lib/github";

export const runtime = "nodejs";

// Compare calls are one GitHub request per branch; keep the scan bounded.
const MAX_COMPARED_BRANCHES = 25;

export async function GET(
  _request: NextRequest,
  context: { params: Promise<{ workspaceId: string }> },
) {
  const user = await currentUser();
  if (!user) {
    return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  }

  const { workspaceId } = await context.params;
  const workspace = getWorkspaceForUser(workspaceId, user.githubUserId);
  if (!workspace) {
    return NextResponse.json({ error: "workspace not found" }, { status: 404 });
  }

  try {
    const knownBranches = new Set(
      listDraftSessionsForWorkspace(workspace.id, user.githubUserId).map(
        (draft) => draft.branch,
      ),
    );
    const branches = (
      await listGitHubBranches({
        token: user.githubToken,
        owner: workspace.owner,
        name: workspace.name,
      })
    ).filter((branch) => branch !== workspace.ref && !knownBranches.has(branch));
    const compared = branches.slice(0, MAX_COMPARED_BRANCHES);
    const prefix = workspace.path === "." ? "" : `${workspace.path}/`;

    const candidates = (
      await Promise.all(
        compared.map(async (branch) => {
          try {
            const comparison = await compareGitHubRefs({
              token: user.githubToken,
              owner: workspace.owner,
              name: workspace.name,
              base: workspace.ref,
              head: branch,
            });
            const workspaceOnly =
              comparison.aheadBy > 0 &&
              comparison.files.length > 0 &&
              comparison.files.every((file) => file.startsWith(prefix));
            return workspaceOnly
              ? [{ branch, aheadBy: comparison.aheadBy, filesChanged: comparison.files.length }]
              : [];
          } catch {
            // a branch that cannot be compared is not a candidate
            return [];
          }
        }),
      )
    ).flat();

    return NextResponse.json({
      candidates,
      scanned: compared.length,
      skipped: branches.length - compared.length,
    });
  } catch (error) {
    const message = githubErrorMessage(error, "Scanning branches");
    return NextResponse.json({ error: message }, { status: 400 });
  }
}
