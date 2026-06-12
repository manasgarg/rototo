import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import {
  getDraftSessionForUser,
  getWorkspaceForUser,
  recordDraftEvent,
} from "@/lib/db";
import {
  createGitHubFile,
  githubErrorMessage,
  listGitHubTree,
  workspaceRepoPath,
} from "@/lib/github";
import { dropLspSessionsForDraft } from "@/lib/lsp-bridge";
import { invalidateStagedWorkspaces } from "@/lib/rototo";
import { draftLintTarget } from "@/lib/workspace-edit";

export const runtime = "nodejs";

type EntityKind =
  | "variables"
  | "qualifiers"
  | "resources"
  | "resource_objects"
  | "schemas"
  | "context"
  | "linters";

type PlannedFile = {
  path: string;
  content: string;
};

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
      kind?: string;
      id?: string;
      resourceId?: string;
      variableType?: string;
    };
    const kind = parseKind(body.kind);
    const id = parseEntityId(body.id);
    const resourceId = parseEntityId(body.resourceId);
    if (!kind || !id || (kind === "resource_objects" && !resourceId)) {
      return NextResponse.json(
        {
          error:
            "kind and id are required; resource object creation also requires resourceId. ids may contain letters, numbers, dot, dash, and underscore",
        },
        { status: 400 },
      );
    }

    const files = entityTemplateFiles({
      kind,
      id,
      resourceId,
      workspacePath: workspace.path,
      variableType: parseVariableType(body.variableType),
    });
    const tree = await listGitHubTree({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
      ref: draft.branch,
    });
    const existing = new Set(tree.filter((entry) => entry.type === "blob").map((entry) => entry.path));
    if (kind === "resource_objects") {
      const resourcePath = workspaceRepoPath(workspace.path, `resources/${resourceId}.toml`);
      if (!existing.has(resourcePath)) {
        return NextResponse.json(
          { error: `resource does not exist: ${resourceId}` },
          { status: 404 },
        );
      }
    }
    const conflict = files.find((file) => existing.has(file.path));
    if (conflict) {
      return NextResponse.json(
        { error: `file already exists: ${conflict.path}` },
        { status: 409 },
      );
    }

    for (const file of files) {
      await createGitHubFile({
        token: user.githubToken,
        owner: workspace.owner,
        name: workspace.name,
        path: file.path,
        branch: draft.branch,
        content: file.content,
        message: `Create ${file.path}`,
      });
    }
    recordDraftEvent({
      draftId: draft.id,
      kind: "entity.created",
      summary: `Created ${kindLabel(kind)} ${id}`,
      detail: { kind, id, files: files.map((file) => file.path) },
    });
    // Staged checkouts of the draft branch go stale after a commit.
    dropLspSessionsForDraft(draft.id);
    invalidateStagedWorkspaces(draftLintTarget(workspace, draft).source);
    return NextResponse.json({ files });
  } catch (error) {
    const message = githubErrorMessage(error, "Creating the draft entity");
    return NextResponse.json({ error: message }, { status: 400 });
  }
}

function entityTemplateFiles(input: {
  kind: EntityKind;
  id: string;
  resourceId: string | null;
  workspacePath: string;
  variableType: string;
}): PlannedFile[] {
  const path = (relativePath: string) => workspaceRepoPath(input.workspacePath, relativePath);
  switch (input.kind) {
    case "variables":
      return [
        {
          path: path(`variables/${input.id}.toml`),
          content: variableTemplate(input.id, input.variableType),
        },
      ];
    case "qualifiers":
      return [
        {
          path: path(`qualifiers/${input.id}.toml`),
          content: qualifierTemplate(input.id),
        },
      ];
    case "resources":
      return [
        {
          path: path(`resources/${input.id}.toml`),
          content: resourceTemplate(input.id),
        },
        {
          path: path(`schemas/${input.id}.schema.json`),
          content: resourceSchemaTemplate(),
        },
        {
          path: path(`resources/${input.id}-objects/default.toml`),
          content: resourceObjectTemplate(),
        },
      ];
    case "resource_objects":
      if (!input.resourceId) {
        throw new Error("resourceId is required to create a resource object");
      }
      return [
        {
          path: path(`resources/${input.resourceId}-objects/${input.id}.toml`),
          content: resourceObjectTemplate(),
        },
      ];
    case "schemas":
      return [
        {
          path: path(`schemas/${jsonFileName(input.id)}`),
          content: schemaTemplate(input.id),
        },
      ];
    case "context":
      return [
        {
          path: path(`contexts/${jsonFileName(input.id)}`),
          content: "{\n}\n",
        },
      ];
    case "linters":
      return [
        {
          path: path(`lint/${input.id}.lua`),
          content: linterTemplate(),
        },
      ];
  }
}

function variableTemplate(id: string, variableType: string): string {
  const defaultLiteral =
    variableType === "bool"
      ? "false"
      : variableType === "int" || variableType === "number"
        ? "0"
        : variableType === "list"
          ? "[]"
          : "\"control\"";
  return [
    "schema_version = 1",
    "",
    `description = ${tomlString(`Edit this description to explain what ${id} controls`)}`,
    `type = ${tomlString(variableType)}`,
    "",
    "[values]",
    `control = ${defaultLiteral}`,
    "",
    "[resolve]",
    "default = \"control\"",
    "",
  ].join("\n");
}

function qualifierTemplate(id: string): string {
  return [
    "schema_version = 1",
    "",
    `description = ${tomlString(`Edit this description to explain when ${id} should match`)}`,
    "",
    "[[predicate]]",
    "attribute = \"user.tier\"",
    "op = \"eq\"",
    "value = \"premium\"",
    "",
  ].join("\n");
}

function resourceTemplate(id: string): string {
  return [
    "schema_version = 1",
    "",
    `description = ${tomlString(`Edit this description to explain the ${id} resource objects`)}`,
    `schema = ${tomlString(`../schemas/${id}.schema.json`)}`,
    "",
  ].join("\n");
}

function resourceSchemaTemplate(): string {
  return `${JSON.stringify(
    {
      $schema: "https://json-schema.org/draft/2020-12/schema",
      type: "object",
      additionalProperties: false,
      properties: {
        heading: { type: "string" },
        enabled: { type: "boolean" },
      },
      required: ["heading", "enabled"],
    },
    null,
    2,
  )}\n`;
}

function resourceObjectTemplate(): string {
  return "heading = \"Edit this heading\"\nenabled = false\n";
}

function schemaTemplate(id: string): string {
  return `${JSON.stringify(
    {
      $schema: "https://json-schema.org/draft/2020-12/schema",
      title: id,
      type: "object",
      additionalProperties: true,
    },
    null,
    2,
  )}\n`;
}

function linterTemplate(): string {
  return [
    "function register(lint)",
    "  -- Register custom lint handlers here.",
    "end",
    "",
  ].join("\n");
}

function parseKind(value: string | undefined): EntityKind | null {
  if (
    value === "variables" ||
    value === "qualifiers" ||
    value === "resources" ||
    value === "resource_objects" ||
    value === "schemas" ||
    value === "context" ||
    value === "linters"
  ) {
    return value;
  }
  return null;
}

function parseEntityId(value: string | undefined): string | null {
  const id = value?.trim();
  return id && /^[A-Za-z0-9_.-]+$/.test(id) ? id : null;
}

function parseVariableType(value: string | undefined): string {
  return value === "bool" ||
    value === "int" ||
    value === "number" ||
    value === "string" ||
    value === "list"
    ? value
    : "string";
}

function jsonFileName(id: string): string {
  return id.endsWith(".json") ? id : `${id}.json`;
}

function kindLabel(kind: EntityKind): string {
  if (kind === "resource_objects") {
    return "resource object";
  }
  return kind === "context" ? "context example" : kind.slice(0, -1);
}

function tomlString(value: string): string {
  return JSON.stringify(value);
}
