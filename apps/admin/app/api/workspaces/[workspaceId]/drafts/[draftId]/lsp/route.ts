import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import { getDraftSessionForUser, getWorkspaceForUser } from "@/lib/db";
import {
  lspCompletion,
  lspHover,
  lspUpdate,
  type LspPositionWire,
} from "@/lib/lsp-bridge";

export const runtime = "nodejs";

export async function POST(
  request: NextRequest,
  context: { params: Promise<{ workspaceId: string; draftId: string }> },
) {
  const user = await currentUser();
  if (!user) {
    return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  }

  const { workspaceId, draftId } = await context.params;
  const workspace = getWorkspaceForUser(workspaceId, user.githubUserId);
  if (!workspace) {
    return NextResponse.json({ error: "workspace not found" }, { status: 404 });
  }
  const draft = getDraftSessionForUser(draftId, workspace.id, user.githubUserId);
  if (!draft) {
    return NextResponse.json({ error: "draft not found" }, { status: 404 });
  }
  if (draft.status !== "open") {
    return NextResponse.json({ error: "draft is already published" }, { status: 400 });
  }

  try {
    const body = (await request.json()) as {
      op?: string;
      path?: string;
      text?: string;
      position?: LspPositionWire;
    };
    const path = body.path?.trim();
    if (!path || typeof body.text !== "string") {
      return NextResponse.json({ error: "path and text are required" }, { status: 400 });
    }
    if (!belongsToWorkspace(workspace.path, path)) {
      return NextResponse.json(
        { error: "file path does not belong to workspace" },
        { status: 400 },
      );
    }
    const common = {
      workspace,
      draft,
      githubToken: user.githubToken,
      userId: user.githubUserId,
      path,
      text: body.text,
    };
    if (body.op === "update") {
      return NextResponse.json(await lspUpdate(common));
    }
    if (body.op === "completion" && body.position) {
      return NextResponse.json(await lspCompletion({ ...common, position: body.position }));
    }
    if (body.op === "hover" && body.position) {
      return NextResponse.json(await lspHover({ ...common, position: body.position }));
    }
    return NextResponse.json({ error: "unknown lsp op" }, { status: 400 });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return NextResponse.json({ error: message }, { status: 400 });
  }
}

function belongsToWorkspace(workspacePath: string, filePath: string): boolean {
  if (filePath.startsWith("/") || filePath.split("/").includes("..")) {
    return false;
  }
  return workspacePath === "." || filePath.startsWith(`${workspacePath}/`);
}
