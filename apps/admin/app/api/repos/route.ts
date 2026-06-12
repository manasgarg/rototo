import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import { listReposForUser, upsertRepoWithWorkspaces } from "@/lib/db";
import {
  discoverGitHubWorkspaces,
  getGitHubRepo,
  parseRepoSpec,
} from "@/lib/github";

export const runtime = "nodejs";

export async function GET() {
  const user = await currentUser();
  if (!user) {
    return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  }
  return NextResponse.json({ repos: listReposForUser(user.githubUserId) });
}

export async function POST(request: NextRequest) {
  const user = await currentUser();
  if (!user) {
    return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  }

  try {
    const body = (await request.json()) as { repo?: string; ref?: string };
    const { owner, name } = parseRepoSpec(body.repo ?? "");
    const repo = await getGitHubRepo(user.githubToken, owner, name);
    const ref = body.ref?.trim() || repo.default_branch;
    const workspaces = await discoverGitHubWorkspaces({
      token: user.githubToken,
      owner,
      name,
      ref,
    });
    const stored = upsertRepoWithWorkspaces({
      githubUserId: user.githubUserId,
      owner: repo.owner.login,
      name: repo.name,
      defaultRef: ref,
      workspaces,
    });
    return NextResponse.json({ repo: stored });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return NextResponse.json({ error: message }, { status: 400 });
  }
}
