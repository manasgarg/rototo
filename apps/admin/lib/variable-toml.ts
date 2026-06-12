export const PRIMITIVE_VARIABLE_TYPES = ["bool", "int", "number", "string", "list"] as const;

export type PrimitiveVariableType = (typeof PRIMITIVE_VARIABLE_TYPES)[number];

export type PrimitiveVariableEdit = {
  id: string;
  filePath: string;
  description: string | null;
  type: PrimitiveVariableType;
  defaultKey: string;
  defaultValue: unknown;
  defaultLiteral: string;
};

export type VariableDefaultUpdate = {
  text: string;
  before: unknown;
  after: unknown;
  beforeLiteral: string;
  afterLiteral: string;
  valueKey: string;
};

type VariableParseResult = {
  id: string;
  description: string | null;
  type: PrimitiveVariableType | null;
  defaultKey: string | null;
  values: Map<string, { literal: string; value: unknown; lineIndex: number }>;
};

export function parsePrimitiveVariableFile(
  filePath: string,
  text: string,
): PrimitiveVariableEdit | null {
  const parsed = parseVariableFile(filePath, text);
  if (!parsed.type || !parsed.defaultKey) {
    return null;
  }
  const defaultValue = parsed.values.get(parsed.defaultKey);
  if (!defaultValue) {
    return null;
  }
  return {
    id: parsed.id,
    filePath,
    description: parsed.description,
    type: parsed.type,
    defaultKey: parsed.defaultKey,
    defaultValue: defaultValue.value,
    defaultLiteral: defaultValue.literal,
  };
}

export function updatePrimitiveVariableDefault(input: {
  filePath: string;
  text: string;
  value: string;
}): VariableDefaultUpdate {
  const parsed = parseVariableFile(input.filePath, input.text);
  if (!parsed.type) {
    throw new Error("Only primitive variables can be edited in this view.");
  }
  if (!parsed.defaultKey) {
    throw new Error("Variable does not declare a resolve default.");
  }
  const existing = parsed.values.get(parsed.defaultKey);
  if (!existing) {
    throw new Error(`Variable default value ${parsed.defaultKey} is not declared under [values].`);
  }

  const after = parseInputValue(input.value, parsed.type);
  const afterLiteral = formatTomlLiteral(after, parsed.type);
  const lines = input.text.split(/\r?\n/);
  lines[existing.lineIndex] = lines[existing.lineIndex].replace(
    /^(\s*[A-Za-z0-9_-]+\s*=\s*).*/,
    `$1${afterLiteral}`,
  );

  return {
    text: lines.join("\n"),
    before: existing.value,
    after,
    beforeLiteral: existing.literal,
    afterLiteral,
    valueKey: parsed.defaultKey,
  };
}

function parseVariableFile(filePath: string, text: string): VariableParseResult {
  const id = filePath.split("/").pop()?.replace(/\.toml$/, "") ?? filePath;
  let description: string | null = null;
  let type: PrimitiveVariableType | null = null;
  let defaultKey: string | null = null;
  const values = new Map<string, { literal: string; value: unknown; lineIndex: number }>();
  let section = "";

  const lines = text.split(/\r?\n/);
  lines.forEach((line, lineIndex) => {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) {
      return;
    }
    if (/^\[\[/.test(trimmed)) {
      section = trimmed;
      return;
    }
    if (/^\[/.test(trimmed)) {
      section = trimmed;
      return;
    }

    if (!section) {
      const typeMatch = /^type\s*=\s*"([^"]+)"\s*$/.exec(trimmed);
      if (typeMatch && isPrimitiveVariableType(typeMatch[1])) {
        type = typeMatch[1];
        return;
      }
      const descriptionMatch = /^description\s*=\s*(".*")\s*$/.exec(trimmed);
      if (descriptionMatch) {
        description = parseTomlString(descriptionMatch[1]);
      }
      return;
    }

    if (section === "[resolve]") {
      const defaultMatch = /^default\s*=\s*"([^"]+)"\s*$/.exec(trimmed);
      if (defaultMatch) {
        defaultKey = defaultMatch[1];
      }
      return;
    }

    if (section === "[values]") {
      const valueMatch = /^([A-Za-z0-9_-]+)\s*=\s*(.+?)\s*$/.exec(trimmed);
      if (!valueMatch) {
        return;
      }
      const literal = valueMatch[2];
      values.set(valueMatch[1], {
        literal,
        value: parseTomlLiteral(literal),
        lineIndex,
      });
    }
  });

  return { id, description, type, defaultKey, values };
}

function isPrimitiveVariableType(value: string): value is PrimitiveVariableType {
  return PRIMITIVE_VARIABLE_TYPES.includes(value as PrimitiveVariableType);
}

function parseInputValue(value: string, type: PrimitiveVariableType): unknown {
  const trimmed = value.trim();
  switch (type) {
    case "bool":
      if (trimmed !== "true" && trimmed !== "false") {
        throw new Error("Boolean values must be true or false.");
      }
      return trimmed === "true";
    case "int": {
      const number = Number(trimmed);
      if (!Number.isInteger(number)) {
        throw new Error("Integer values must be whole numbers.");
      }
      return number;
    }
    case "number": {
      const number = Number(trimmed);
      if (!Number.isFinite(number)) {
        throw new Error("Number values must be finite.");
      }
      return number;
    }
    case "string":
      return trimmed;
    case "list": {
      const parsed = JSON.parse(trimmed) as unknown;
      if (!Array.isArray(parsed)) {
        throw new Error("List values must be a JSON array.");
      }
      return parsed;
    }
  }
}

function parseTomlLiteral(literal: string): unknown {
  const trimmed = literal.trim();
  if (trimmed.startsWith('"')) {
    return parseTomlString(trimmed);
  }
  if (trimmed === "true") {
    return true;
  }
  if (trimmed === "false") {
    return false;
  }
  if (trimmed.startsWith("[")) {
    return JSON.parse(trimmed) as unknown;
  }
  const number = Number(trimmed.replace(/_/g, ""));
  if (Number.isFinite(number)) {
    return number;
  }
  return trimmed;
}

function parseTomlString(literal: string): string {
  return JSON.parse(literal) as string;
}

function formatTomlLiteral(value: unknown, type: PrimitiveVariableType): string {
  switch (type) {
    case "bool":
      return value ? "true" : "false";
    case "int":
    case "number":
      return String(value);
    case "string":
      return JSON.stringify(value);
    case "list":
      return JSON.stringify(value);
  }
}
