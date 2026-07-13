// Shared syntax-highlighted source view. This follows the CodeMirror setup
// already used by the console in ../3: JSON gets its Lezer grammar, TOML gets
// the maintained legacy mode, and both use the rototo palette.
//
// With an `lsp` handle (lib/lsp.ts) the editor becomes a language client:
// the buffer streams to the bridge as it changes, published diagnostics
// render as squiggles plus line tints, completion and hover ask the real
// rototo language server, and TOML blank lines and CEL expressions open the
// completion list on their own.

import CodeMirror from "@uiw/react-codemirror";
import {
    autocompletion,
    completionKeymap,
    startCompletion,
    type CompletionContext,
    type CompletionResult,
} from "@codemirror/autocomplete";
import { json } from "@codemirror/lang-json";
import {
    HighlightStyle,
    StreamLanguage,
    syntaxHighlighting,
} from "@codemirror/language";
import { lua } from "@codemirror/legacy-modes/mode/lua";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import {
    RangeSetBuilder,
    StateEffect,
    StateField,
    type Extension,
    type Text,
} from "@codemirror/state";
import {
    Decoration,
    EditorView,
    hoverTooltip,
    keymap,
    ViewPlugin,
    type DecorationSet,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { useMemo, type KeyboardEventHandler } from "react";

import type { LspFile, LspPosition } from "@/lib/lsp";

export type CodeEditorLanguage = "json" | "lua" | "toml" | "text";

const rototoHighlight = HighlightStyle.define([
    { tag: tags.comment, color: "var(--ink-3)", fontStyle: "italic" },
    { tag: [tags.string, tags.special(tags.string)], color: "var(--sea-700)" },
    {
        tag: [tags.number, tags.bool, tags.atom, tags.null],
        color: "var(--info-700)",
    },
    {
        tag: [tags.keyword, tags.heading, tags.className, tags.typeName],
        color: "var(--sea-600)",
        fontWeight: "600",
    },
    {
        tag: [
            tags.propertyName,
            tags.definition(tags.variableName),
            tags.attributeName,
        ],
        color: "var(--ink-0)",
        fontWeight: "600",
    },
    { tag: [tags.variableName, tags.name], color: "var(--ink-1)" },
    {
        tag: [tags.operator, tags.punctuation, tags.separator, tags.bracket],
        color: "var(--ink-2)",
    },
    { tag: tags.meta, color: "var(--ink-2)" },
]);

export function CodeEditor({
    className,
    disabled = false,
    language,
    lsp,
    onChange,
    onKeyDown,
    value,
}: {
    className?: string;
    disabled?: boolean;
    language: CodeEditorLanguage;
    /** The language-server handle for this buffer's file, when editing. */
    lsp?: LspFile | null;
    onChange: (value: string) => void;
    onKeyDown?: KeyboardEventHandler<HTMLDivElement>;
    value: string;
}) {
    const extensions = useMemo(
        () => [
            ...extensionsForLanguage(language),
            syntaxHighlighting(rototoHighlight),
            ...(lsp != null ? lspExtensions(lsp, language) : []),
        ],
        [language, lsp],
    );

    return (
        <div
            className={`code-editor-shell${className === undefined ? "" : ` ${className}`}`}
            onKeyDown={onKeyDown}
        >
            <CodeMirror
                basicSetup={{
                    autocompletion: lsp == null,
                    bracketMatching: true,
                    completionKeymap: true,
                    foldGutter: true,
                    highlightActiveLine: !disabled,
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

export function codeLanguageForPath(path: string): CodeEditorLanguage {
    if (path.endsWith(".json")) {
        return "json";
    }
    if (path.endsWith(".toml")) {
        return "toml";
    }
    if (path.endsWith(".lua")) {
        return "lua";
    }
    return "text";
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

/* --- the language-server client half (pattern from ../3's editor) --- */

type LineMark = { line: number; severity: "error" | "warning" };

const setDiagnosticMarks = StateEffect.define<LineMark[]>();

/* Tint the lines lint pointed at; positions follow the text as it edits. */
const diagnosticLineField = StateField.define<DecorationSet>({
    create() {
        return Decoration.none;
    },
    update(value, transaction) {
        let decorations = transaction.docChanged
            ? value.map(transaction.changes)
            : value;
        for (const effect of transaction.effects) {
            if (effect.is(setDiagnosticMarks)) {
                decorations = markDecorations(
                    transaction.state.doc,
                    effect.value,
                );
            }
        }
        return decorations;
    },
    provide: (field) => EditorView.decorations.from(field),
});

function markDecorations(doc: Text, marks: LineMark[]): DecorationSet {
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
                class:
                    mark.severity === "error"
                        ? "cm-diag-error"
                        : "cm-diag-warning",
            }),
        );
    }
    return builder.finish();
}

function lspExtensions(
    lsp: LspFile,
    language: CodeEditorLanguage,
): Extension[] {
    return [
        diagnosticLineField,
        lspDiagnosticsPlugin(lsp),
        autocompletion({ override: [lspCompletionSource(lsp, language)] }),
        keymap.of(completionKeymap),
        lspHoverTooltip(lsp),
        ...(language === "toml"
            ? [tomlBlankLineCompletionActivator(), celCompletionActivator()]
            : []),
    ];
}

/* Stream the buffer to the server overlay and render what it publishes:
   squiggles with messages, plus a tint on each diagnosed line. */
function lspDiagnosticsPlugin(lsp: LspFile): Extension {
    return ViewPlugin.define((view) => {
        lsp.update(view.state.doc.toString());
        const unsubscribe = lsp.onDiagnostics((published) => {
            const doc = view.state.doc;
            const diagnostics: Diagnostic[] = published.map((item) => {
                const from = positionToOffset(doc, item.range.start);
                return {
                    from,
                    to: Math.max(from, positionToOffset(doc, item.range.end)),
                    severity: item.severity === 1 ? "error" : "warning",
                    message:
                        item.data?.help !== undefined
                            ? `${item.message}\n${item.data.help}`
                            : item.message,
                    source: item.code,
                };
            });
            const tints: LineMark[] = published.map((item) => ({
                line: Math.min(item.range.start.line + 1, doc.lines),
                severity: item.severity === 1 ? "error" : "warning",
            }));
            view.dispatch(setDiagnostics(view.state, diagnostics), {
                effects: setDiagnosticMarks.of(tints),
            });
        });
        return {
            update(update) {
                if (update.docChanged) {
                    lsp.update(update.state.doc.toString());
                }
            },
            destroy() {
                unsubscribe();
            },
        };
    });
}

function lspCompletionSource(lsp: LspFile, language: CodeEditorLanguage) {
    return async (
        context: CompletionContext,
    ): Promise<CompletionResult | null> => {
        const word = context.matchBefore(/[\w.-]+/);
        const operator = context.matchBefore(/[&|]+/);
        if (!shouldRequestLspCompletion(context, word, language)) {
            return null;
        }
        let items;
        try {
            items = await lsp.completion(
                offsetToPosition(context.state.doc, context.pos),
            );
        } catch {
            return null;
        }
        if (items.length === 0) {
            return null;
        }
        const allOperators = items.every((item) => item.kind === 24);
        return {
            from:
                allOperators && operator
                    ? operator.from
                    : word
                      ? word.from
                      : context.pos,
            validFor: allOperators ? /^[&|]*$/ : /^[\w.[\]-]*$/,
            options: items.map((item, index) => ({
                label: item.label,
                type: completionType(item.kind ?? 1),
                detail: item.detail,
                apply: item.insertText ?? item.label,
                boost: items.length - index,
            })),
        };
    };
}

function shouldRequestLspCompletion(
    context: CompletionContext,
    word: ReturnType<CompletionContext["matchBefore"]>,
    language: CodeEditorLanguage,
): boolean {
    return (
        context.explicit ||
        word !== null ||
        (language === "toml" &&
            cursorLooksAtBlankTomlLine(context.state.doc, context.pos)) ||
        cursorLooksInsideCelExpression(context.state.doc, context.pos)
    );
}

/* Typing inside a `when`/`query` CEL string reopens the list after a
   connective or a space, where the next identifier starts. */
function celCompletionActivator(): Extension {
    return EditorView.updateListener.of((update) => {
        if (!update.docChanged || !update.view.hasFocus) {
            return;
        }
        const selection = update.state.selection.main;
        if (
            !selection.empty ||
            !cursorLooksInsideCelExpression(update.state.doc, selection.head)
        ) {
            return;
        }
        const line = update.state.doc.lineAt(selection.head);
        const beforeCursor = line.text.slice(0, selection.head - line.from);
        if (!/[&|\s]$/.test(beforeCursor)) {
            return;
        }
        globalThis.setTimeout(() => {
            const selection = update.view.state.selection.main;
            if (
                update.view.hasFocus &&
                selection.empty &&
                cursorLooksInsideCelExpression(
                    update.view.state.doc,
                    selection.head,
                )
            ) {
                startCompletion(update.view);
            }
        }, 0);
    });
}

/* A blank TOML line is where the next key or table goes; offer them. */
function tomlBlankLineCompletionActivator(): Extension {
    return EditorView.updateListener.of((update) => {
        if (!update.docChanged || !update.view.hasFocus) {
            return;
        }
        const selection = update.state.selection.main;
        if (
            !selection.empty ||
            !cursorLooksAtBlankTomlLine(update.state.doc, selection.head)
        ) {
            return;
        }
        globalThis.setTimeout(() => {
            if (
                update.view.hasFocus &&
                cursorLooksAtBlankTomlLine(
                    update.view.state.doc,
                    update.view.state.selection.main.head,
                )
            ) {
                startCompletion(update.view);
            }
        }, 0);
    });
}

function cursorLooksAtBlankTomlLine(doc: Text, offset: number): boolean {
    const line = doc.lineAt(offset);
    const beforeCursor = line.text.slice(0, offset - line.from);
    return beforeCursor.trim().length === 0;
}

function cursorLooksInsideCelExpression(doc: Text, offset: number): boolean {
    const line = doc.lineAt(offset);
    const beforeCursor = line.text.slice(0, offset - line.from);
    const equals = beforeCursor.indexOf("=");
    if (equals < 0) {
        return false;
    }
    const key = beforeCursor
        .slice(0, equals)
        .trimEnd()
        .match(/([\w-]+)$/)?.[1];
    if (key !== "when" && key !== "query" && key !== "filter") {
        return false;
    }
    const valuePrefix = beforeCursor.slice(equals + 1).trimStart();
    const quote = valuePrefix[0];
    if (quote !== `"` && quote !== `'`) {
        return false;
    }
    return !containsClosingTomlStringQuote(valuePrefix.slice(1), quote);
}

function containsClosingTomlStringQuote(value: string, quote: string): boolean {
    let escaped = false;
    for (const character of value) {
        if (quote === `"` && escaped) {
            escaped = false;
            continue;
        }
        if (quote === `"` && character === "\\") {
            escaped = true;
            continue;
        }
        if (character === quote) {
            return true;
        }
        escaped = false;
    }
    return false;
}

function lspHoverTooltip(lsp: LspFile): Extension {
    return hoverTooltip(async (view, pos) => {
        let hover;
        try {
            hover = await lsp.hover(offsetToPosition(view.state.doc, pos));
        } catch {
            return null;
        }
        if (hover === null || hover.value === "") {
            return null;
        }
        const from = hover.range
            ? positionToOffset(view.state.doc, hover.range.start)
            : pos;
        const to = hover.range
            ? positionToOffset(view.state.doc, hover.range.end)
            : pos;
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
        case 3:
            return "function";
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
