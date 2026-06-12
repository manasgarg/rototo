"use client";

import CodeMirror from "@uiw/react-codemirror";
import {
  autocompletion,
  completionKeymap,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import { json } from "@codemirror/lang-json";
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { lua } from "@codemirror/legacy-modes/mode/lua";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import { RangeSetBuilder, StateEffect, StateField, type Extension, type Text } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  hoverTooltip,
  keymap,
  ViewPlugin,
  type DecorationSet,
} from "@codemirror/view";
import { useMemo } from "react";

export type CodeEditorLanguage = "json" | "lua" | "toml" | "text";

/* Syntax colors from the rototo palette: ink for structure, sea green for
   values, cyan rationed to literals. */
const rototoHighlight = HighlightStyle.define([
  { tag: tags.comment, color: "var(--ink-3)", fontStyle: "italic" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--sea-700)" },
  { tag: [tags.number, tags.bool, tags.atom, tags.null], color: "var(--info-700)" },
  { tag: [tags.keyword, tags.heading, tags.className, tags.typeName], color: "var(--sea-600)", fontWeight: "600" },
  { tag: [tags.propertyName, tags.definition(tags.variableName), tags.attributeName], color: "var(--ink-0)", fontWeight: "600" },
  { tag: [tags.variableName, tags.name], color: "var(--ink-1)" },
  { tag: [tags.operator, tags.punctuation, tags.separator, tags.bracket], color: "var(--ink-2)" },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: "var(--sea-600)" },
  { tag: tags.meta, color: "var(--ink-2)" },
]);

export type CodeEditorMark = {
  line: number;
  severity: "error" | "warning";
};

type LspPosition = { line: number; character: number };

type LspRange = { start: LspPosition; end: LspPosition };

type LspUpdateResponse = {
  diagnostics?: Array<{
    message: string;
    severity: "error" | "warning";
    rule?: string | null;
    help?: string | null;
    range: LspRange;
  }>;
};

type LspCompletionResponse = {
  items?: Array<{ label: string; kind: number; detail?: string | null }>;
};

type LspHoverResponse = {
  hover?: { value: string; range?: LspRange | null } | null;
};

/* Wires the editor to the draft's rototo language server session. `request`
   posts one bridge op and resolves with its JSON body. */
export type CodeEditorLsp = {
  request: (body: Record<string, unknown>) => Promise<unknown>;
};

export function CodeEditor({
  disabled,
  language,
  lsp,
  marks,
  onChange,
  value,
}: {
  disabled?: boolean;
  language: CodeEditorLanguage;
  lsp?: CodeEditorLsp;
  marks?: CodeEditorMark[];
  onChange: (value: string) => void;
  value: string;
}) {
  const extensions = useMemo(
    () => [
      ...extensionsForLanguage(language),
      syntaxHighlighting(rototoHighlight),
      ...((marks && marks.length > 0) || lsp ? [diagnosticLineField(marks ?? [])] : []),
      ...(lsp ? lspExtensions(lsp) : []),
    ],
    [language, marks, lsp],
  );

  return (
    <div className="code-editor-shell">
      <CodeMirror
        basicSetup={{
          autocompletion: !lsp,
          bracketMatching: true,
          completionKeymap: true,
          foldGutter: true,
          highlightActiveLine: true,
          lineNumbers: true,
        }}
        editable={!disabled}
        extensions={extensions}
        onChange={onChange}
        theme="light"
        value={value}
      />
    </div>
  );
}

const setDiagnosticMarks = StateEffect.define<CodeEditorMark[]>();

/* Tint the lines lint pointed at; positions follow the text as it is edited.
   Live LSP responses replace the initial server-rendered marks. */
function diagnosticLineField(initial: CodeEditorMark[]): Extension {
  return StateField.define<DecorationSet>({
    create(state) {
      return markDecorations(state.doc, initial);
    },
    update(value, transaction) {
      let decorations = transaction.docChanged ? value.map(transaction.changes) : value;
      for (const effect of transaction.effects) {
        if (effect.is(setDiagnosticMarks)) {
          decorations = markDecorations(transaction.state.doc, effect.value);
        }
      }
      return decorations;
    },
    provide: (field) => EditorView.decorations.from(field),
  });
}

function markDecorations(doc: Text, marks: CodeEditorMark[]): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const sorted = [...marks].sort((left, right) => left.line - right.line);
  const seen = new Set<number>();
  for (const mark of sorted) {
    if (mark.line < 1 || mark.line > doc.lines || seen.has(mark.line)) {
      continue;
    }
    seen.add(mark.line);
    const line = doc.line(mark.line);
    builder.add(
      line.from,
      line.from,
      Decoration.line({
        class: mark.severity === "error" ? "cm-diag-error" : "cm-diag-warning",
      }),
    );
  }
  return builder.finish();
}

function lspExtensions(lsp: CodeEditorLsp): Extension[] {
  // Shared between the diagnostics plugin and hover: the lint tooltip already
  // explains diagnostics on hover, so the LSP hover skips those ranges.
  const lastDiagnostics: { ranges: Array<{ from: number; to: number }>; doc: Text | null } = {
    ranges: [],
    doc: null,
  };
  return [
    lspDiagnosticsPlugin(lsp, lastDiagnostics),
    autocompletion({ override: [lspCompletionSource(lsp)] }),
    keymap.of(completionKeymap),
    lspHoverTooltip(lsp, lastDiagnostics),
  ];
}

/* As-you-type diagnostics: debounce, send the full document, and apply both
   squiggles (with messages) and line tints when the response still matches
   the current text. */
function lspDiagnosticsPlugin(
  lsp: CodeEditorLsp,
  lastDiagnostics: { ranges: Array<{ from: number; to: number }>; doc: Text | null },
): Extension {
  return ViewPlugin.define((view) => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    let destroyed = false;

    const run = async () => {
      const text = view.state.doc.toString();
      let response: LspUpdateResponse;
      try {
        response = (await lsp.request({ op: "update", text })) as LspUpdateResponse;
      } catch {
        return;
      }
      if (destroyed || view.state.doc.toString() !== text) {
        return;
      }
      const doc = view.state.doc;
      const found = response.diagnostics ?? [];
      const diagnostics: Diagnostic[] = found.map((item) => {
        const from = positionToOffset(doc, item.range.start);
        return {
          from,
          to: Math.max(from, positionToOffset(doc, item.range.end)),
          severity: item.severity,
          message: item.help ? `${item.message}\n${item.help}` : item.message,
          source: item.rule ?? undefined,
        };
      });
      const tints: CodeEditorMark[] = found.map((item) => ({
        line: Math.min(item.range.start.line + 1, doc.lines),
        severity: item.severity,
      }));
      lastDiagnostics.ranges = diagnostics.map((item) => ({ from: item.from, to: item.to }));
      lastDiagnostics.doc = doc;
      view.dispatch(setDiagnostics(view.state, diagnostics), {
        effects: setDiagnosticMarks.of(tints),
      });
    };

    const schedule = (delay: number) => {
      if (timer !== null) {
        clearTimeout(timer);
      }
      timer = setTimeout(() => void run(), delay);
    };
    schedule(300);

    return {
      update(update) {
        if (update.docChanged) {
          schedule(500);
        }
      },
      destroy() {
        destroyed = true;
        if (timer !== null) {
          clearTimeout(timer);
        }
      },
    };
  });
}

function lspCompletionSource(lsp: CodeEditorLsp) {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const word = context.matchBefore(/[\w.-]+/);
    if (!context.explicit && !word) {
      return null;
    }
    let response: LspCompletionResponse;
    try {
      response = (await lsp.request({
        op: "completion",
        text: context.state.doc.toString(),
        position: offsetToPosition(context.state.doc, context.pos),
      })) as LspCompletionResponse;
    } catch {
      return null;
    }
    const items = response.items ?? [];
    if (items.length === 0) {
      return null;
    }
    return {
      from: word ? word.from : context.pos,
      validFor: /^[\w.-]*$/,
      options: items.map((item) => ({
        label: item.label,
        type: completionType(item.kind),
        detail: item.detail ?? undefined,
      })),
    };
  };
}

function lspHoverTooltip(
  lsp: CodeEditorLsp,
  lastDiagnostics: { ranges: Array<{ from: number; to: number }>; doc: Text | null },
): Extension {
  return hoverTooltip(async (view, pos) => {
    if (
      lastDiagnostics.doc === view.state.doc &&
      lastDiagnostics.ranges.some((range) => pos >= range.from && pos <= range.to)
    ) {
      return null;
    }
    let response: LspHoverResponse;
    try {
      response = (await lsp.request({
        op: "hover",
        text: view.state.doc.toString(),
        position: offsetToPosition(view.state.doc, pos),
      })) as LspHoverResponse;
    } catch {
      return null;
    }
    const hover = response.hover;
    if (!hover?.value) {
      return null;
    }
    const from = hover.range ? positionToOffset(view.state.doc, hover.range.start) : pos;
    const to = hover.range ? positionToOffset(view.state.doc, hover.range.end) : pos;
    return {
      pos: from,
      end: Math.max(from, to),
      create() {
        const dom = document.createElement("div");
        dom.className = "cm-rototo-hover";
        dom.textContent = hoverText(hover.value);
        return { dom };
      },
    };
  });
}

/* Hover contents arrive as markdown; render as text without the syntax. */
function hoverText(value: string): string {
  return value
    .split("\n")
    .map((line) => line.replace(/^#{1,6}\s*/, "").replace(/`/g, ""))
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function completionType(kind: number): string {
  switch (kind) {
    case 18:
      return "namespace";
    case 12:
      return "constant";
    case 24:
      return "keyword";
    case 5:
      return "property";
    default:
      return "text";
  }
}

function offsetToPosition(doc: Text, offset: number): LspPosition {
  const line = doc.lineAt(offset);
  return { line: line.number - 1, character: offset - line.from };
}

function positionToOffset(doc: Text, position: LspPosition): number {
  if (position.line >= doc.lines) {
    return doc.length;
  }
  const line = doc.line(position.line + 1);
  return Math.min(line.from + position.character, line.to);
}

function extensionsForLanguage(language: CodeEditorLanguage): Extension[] {
  switch (language) {
    case "json":
      return [json()];
    case "lua":
      return [StreamLanguage.define(lua)];
    case "toml":
      return [StreamLanguage.define(toml)];
    case "text":
      return [];
  }
}
