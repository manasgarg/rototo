// Presentation-level reads of entity definition files for the inspect
// screens. This mirrors the structure of `rototo inspect` output without
// reimplementing any resolution semantics: it only reports what the file
// declares, in declaration order.

export type VariableRuleSummary = {
  index: number;
  qualifier: string | null;
  value: string | null;
};

export type VariableSummary = {
  defaultKey: string | null;
  rules: VariableRuleSummary[];
  values: Array<{ key: string; literal: string }>;
};

export type QualifierPredicateSummary = {
  index: number;
  subject: string | null;
  op: string | null;
  valueLiteral: string | null;
};

export type SchemaPropertySummary = {
  key: string;
  type: string | null;
  required: boolean;
  description: string | null;
};

export type SchemaSummary = {
  title: string | null;
  type: string | null;
  properties: SchemaPropertySummary[];
  additionalProperties: boolean | null;
};

export function topLevelFields(text: string): Array<{ key: string; literal: string }> {
  return tomlBlocks(text)[0].fields;
}

export function variableSummary(text: string): VariableSummary {
  const blocks = tomlBlocks(text);
  const resolve = blocks.find((block) => block.header === "resolve");
  const values = blocks.find((block) => block.header === "values");
  const rules = blocks
    .filter((block) => block.header === "resolve.rule")
    .map((block, index) => ({
      index,
      qualifier: stringLiteral(blockField(block, "qualifier")),
      value: stringLiteral(blockField(block, "value")),
    }));
  return {
    defaultKey: resolve ? stringLiteral(blockField(resolve, "default")) : null,
    rules,
    values: values ? values.fields : [],
  };
}

export function qualifierSummary(text: string): QualifierPredicateSummary[] {
  return tomlBlocks(text)
    .filter((block) => block.header === "predicate")
    .map((block, index) => {
      const attribute = stringLiteral(blockField(block, "attribute"));
      const qualifierRef = stringLiteral(blockField(block, "qualifier"));
      return {
        index,
        subject: attribute ?? (qualifierRef ? `qualifier.${qualifierRef}` : null),
        op: stringLiteral(blockField(block, "op")),
        valueLiteral: blockField(block, "value"),
      };
    });
}

export function schemaSummary(text: string): SchemaSummary | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  if (!isRecord(parsed)) {
    return null;
  }
  const required = Array.isArray(parsed.required)
    ? parsed.required.filter((value): value is string => typeof value === "string")
    : [];
  const properties = isRecord(parsed.properties)
    ? Object.entries(parsed.properties).map(([key, definition]) => {
        const record = isRecord(definition) ? definition : {};
        return {
          key,
          type: typeof record.type === "string" ? record.type : null,
          required: required.includes(key),
          description:
            typeof record.description === "string" ? record.description : null,
        };
      })
    : [];
  return {
    title: typeof parsed.title === "string" ? parsed.title : null,
    type: typeof parsed.type === "string" ? parsed.type : null,
    properties,
    additionalProperties:
      typeof parsed.additionalProperties === "boolean"
        ? parsed.additionalProperties
        : null,
  };
}

type TomlBlock = {
  header: string;
  fields: Array<{ key: string; literal: string }>;
};

function tomlBlocks(text: string): TomlBlock[] {
  const blocks: TomlBlock[] = [{ header: "", fields: [] }];
  for (const line of text.split(/\r?\n/)) {
    const header = /^\s*\[\[?([^\]]+)\]\]?\s*$/.exec(line);
    if (header) {
      blocks.push({ header: header[1].trim(), fields: [] });
      continue;
    }
    const field = /^\s*([A-Za-z0-9_-]+)\s*=\s*(.+?)\s*$/.exec(line);
    if (field) {
      blocks[blocks.length - 1].fields.push({ key: field[1], literal: field[2] });
    }
  }
  return blocks;
}

function blockField(block: TomlBlock, key: string): string | null {
  return block.fields.find((field) => field.key === key)?.literal ?? null;
}

function stringLiteral(literal: string | null): string | null {
  if (literal === null) {
    return null;
  }
  const trimmed = literal.trim();
  if (trimmed.startsWith('"')) {
    try {
      return JSON.parse(trimmed) as string;
    } catch {
      return trimmed.replace(/^"|"$/g, "");
    }
  }
  return trimmed;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
