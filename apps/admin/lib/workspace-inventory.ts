import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import type { Workspace as RototoWorkspace, WorkspaceSemanticModel } from "rototo";
import type { WorkspaceRecord } from "./db";
import { workspaceRepoPath } from "./github";
import { semanticModelFor } from "./rototo";

/* The inventory derives from rototo's semantic model — the admin app does
   not parse workspace files itself. Only context examples are enumerated
   from the contexts/ directory, which is file listing, not parsing. */

export type WorkspaceInventory = {
  variables: VariableInventoryItem[];
  qualifiers: QualifierInventoryItem[];
  resources: ResourceInventoryItem[];
  resourceObjects: ResourceObjectInventoryItem[];
  schemas: SchemaInventoryItem[];
  linters: LinterInventoryItem[];
  context: ContextInventory;
};

export type VariableInventoryItem = {
  id: string;
  path: string;
  description: string | null;
  declaration: string;
  defaultValueKey: string | null;
  ruleCount: number;
  qualifierReferences: string[];
  /* Distinct value keys selected by rules; for resource-typed variables these
     name resource objects. */
  ruleValueKeys: string[];
  resourceReference: string | null;
  schemaReference: string | null;
};

export type QualifierInventoryItem = {
  id: string;
  path: string;
  description: string | null;
  predicateCount: number;
  qualifierReferences: string[];
};

export type ResourceInventoryItem = {
  id: string;
  path: string;
  description: string | null;
  schema: string | null;
  schemaReference: string | null;
  objectCount: number;
};

export type ResourceObjectInventoryItem = {
  resourceId: string;
  key: string;
  id: string;
  path: string;
};

export type SchemaInventoryItem = {
  id: string;
  path: string;
  title: string | null;
};

export type LinterInventoryItem = {
  id: string;
  title: string | null;
  path: string | null;
  kind: "rule" | "script";
};

export type ContextInventory = {
  schemaPath: string | null;
  exampleCount: number;
  examples: string[];
};

export type WorkspaceDefinition = {
  path: string;
  text: string;
  language: "json" | "lua" | "toml" | "text";
};

export async function inspectWorkspaceInventory(input: {
  workspace: WorkspaceRecord;
  inspected: RototoWorkspace;
}): Promise<WorkspaceInventory> {
  const [model, context] = await Promise.all([
    semanticModelFor(input.inspected),
    inspectContext(input.workspace, input.inspected.root),
  ]);
  return inventoryFromModel(input.workspace, model, context);
}

export async function readWorkspaceDefinition(input: {
  workspace: WorkspaceRecord;
  root: string;
  path: string;
}): Promise<WorkspaceDefinition> {
  const localPath = workspaceLocalPath(input.workspace, input.path);
  const text = await readFile(join(input.root, localPath), "utf8");
  return {
    path: input.path,
    text,
    language: languageForPath(input.path),
  };
}

function inventoryFromModel(
  workspace: WorkspaceRecord,
  model: WorkspaceSemanticModel,
  context: ContextInventory,
): WorkspaceInventory {
  const repoPath = (path: string) => workspaceRepoPath(workspace.path, path);

  const variables = model.variables.map((variable) => {
    const rules = variable.resolve?.rules ?? [];
    return {
      id: variable.id,
      path: repoPath(variable.location.path),
      description: variable.description ?? null,
      declaration: declarationLabel(variable.declaration),
      defaultValueKey: variable.resolve?.default?.value ?? null,
      ruleCount: rules.length,
      qualifierReferences: distinctSorted(
        rules.map((rule) => rule.qualifier?.value).filter(isString),
      ),
      ruleValueKeys: distinctSorted(rules.map((rule) => rule.value?.value).filter(isString)),
      resourceReference:
        variable.declaration.kind === "resource" ? variable.declaration.value ?? null : null,
      schemaReference:
        variable.declaration.kind === "schema"
          ? variable.declaration.value?.split("/").pop() ?? null
          : null,
    };
  });

  const qualifierEdges = new Map<string, string[]>();
  for (const reference of model.references) {
    if (reference.from.kind === "qualifier" && reference.to.kind === "qualifier") {
      const existing = qualifierEdges.get(reference.from.id) ?? [];
      existing.push(reference.to.id);
      qualifierEdges.set(reference.from.id, existing);
    }
  }
  const qualifiers = model.qualifiers.map((qualifier) => ({
    id: qualifier.id,
    path: repoPath(qualifier.location.path),
    description: qualifier.description ?? null,
    predicateCount: qualifier.predicates.length,
    qualifierReferences: distinctSorted(qualifierEdges.get(qualifier.id) ?? []),
  }));

  const objectCounts = new Map<string, number>();
  for (const object of model.resourceObjects) {
    objectCounts.set(object.resource, (objectCounts.get(object.resource) ?? 0) + 1);
  }
  const resources = model.resources.map((resource) => ({
    id: resource.id,
    path: repoPath(resource.location.path),
    description: resource.description ?? null,
    schema: resource.schema?.value ?? null,
    schemaReference: resource.schema?.value?.split("/").pop() ?? null,
    objectCount: objectCounts.get(resource.id) ?? 0,
  }));

  const resourceObjects = model.resourceObjects.map((object) => ({
    resourceId: object.resource,
    key: object.key,
    id: `${object.resource}/${object.key}`,
    path: repoPath(object.location.path),
  }));

  const schemas = model.schemas
    .filter((schema) => !schema.path.endsWith("context.schema.json"))
    .map((schema) => {
      const json = schema.json as { title?: unknown; $id?: unknown } | undefined;
      return {
        id: schema.path.split("/").pop() ?? schema.path,
        path: repoPath(schema.path),
        title:
          typeof json?.title === "string"
            ? json.title
            : typeof json?.$id === "string"
              ? json.$id
              : null,
      };
    });

  const linters: LinterInventoryItem[] = model.linters.map((linter) => ({
    id: stem(linter.path.split("/").pop() ?? linter.path),
    title:
      linter.rules.length > 0
        ? Array.from(new Set(linter.rules.map((rule) => rule.title))).join(" · ")
        : null,
    path: repoPath(linter.path),
    kind: "script" as const,
  }));

  return { variables, qualifiers, resources, resourceObjects, schemas, linters, context };
}

function declarationLabel(declaration: WorkspaceSemanticModel["variables"][number]["declaration"]): string {
  switch (declaration.kind) {
    case "primitive":
      return declaration.value ?? "undeclared";
    case "resource":
      return `resource:${declaration.value ?? "?"}`;
    case "schema":
      return `schema:${declaration.value ?? "?"}`;
    case "missing":
      return "undeclared";
    default:
      return declaration.kind;
  }
}

async function inspectContext(
  workspace: WorkspaceRecord,
  root: string,
): Promise<ContextInventory> {
  const schemaEntries = await safeReadDir(join(root, "schemas"));
  const hasContextSchema = schemaEntries.some(
    (entry) => entry.isFile() && entry.name === "context.schema.json",
  );
  const examples = (await safeReadDir(join(root, "contexts")))
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => workspaceRepoPath(workspace.path, `contexts/${entry.name}`))
    .sort((left, right) => left.localeCompare(right));
  return {
    schemaPath: hasContextSchema
      ? workspaceRepoPath(workspace.path, "schemas/context.schema.json")
      : null,
    exampleCount: examples.length,
    examples,
  };
}

function distinctSorted(values: string[]): string[] {
  return Array.from(new Set(values)).sort((left, right) => left.localeCompare(right));
}

function isString(value: string | undefined): value is string {
  return typeof value === "string";
}

function stem(name: string): string {
  return name.replace(/\.[^.]+$/, "");
}

function workspaceLocalPath(workspace: WorkspaceRecord, path: string): string {
  if (path.startsWith("/") || path.split("/").includes("..")) {
    throw new Error("workspace definition path must stay inside the workspace");
  }
  if (workspace.path === ".") {
    return path;
  }
  const prefix = `${workspace.path}/`;
  if (!path.startsWith(prefix)) {
    throw new Error("workspace definition path does not belong to this workspace");
  }
  return path.slice(prefix.length);
}

function languageForPath(path: string): WorkspaceDefinition["language"] {
  if (path.endsWith(".toml")) {
    return "toml";
  }
  if (path.endsWith(".json")) {
    return "json";
  }
  if (path.endsWith(".lua")) {
    return "lua";
  }
  return "text";
}

async function safeReadDir(path: string) {
  try {
    return await readdir(path, { withFileTypes: true });
  } catch (error) {
    if (isNotFoundError(error)) {
      return [];
    }
    throw error;
  }
}

function isNotFoundError(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  );
}
