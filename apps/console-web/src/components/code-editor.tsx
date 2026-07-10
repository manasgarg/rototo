// Shared syntax-highlighted source view. This follows the CodeMirror setup
// already used by the console in ../3: JSON gets its Lezer grammar, TOML gets
// the maintained legacy mode, and both use the rototo palette.

import CodeMirror from "@uiw/react-codemirror";
import { json } from "@codemirror/lang-json";
import {
    HighlightStyle,
    StreamLanguage,
    syntaxHighlighting,
} from "@codemirror/language";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import type { Extension } from "@codemirror/state";
import { tags } from "@lezer/highlight";
import { useMemo, type KeyboardEventHandler } from "react";

export type CodeEditorLanguage = "json" | "toml" | "text";

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
    onChange,
    onKeyDown,
    value,
}: {
    className?: string;
    disabled?: boolean;
    language: CodeEditorLanguage;
    onChange: (value: string) => void;
    onKeyDown?: KeyboardEventHandler<HTMLDivElement>;
    value: string;
}) {
    const extensions = useMemo(
        () => [
            ...extensionsForLanguage(language),
            syntaxHighlighting(rototoHighlight),
        ],
        [language],
    );

    return (
        <div
            className={`code-editor-shell${className === undefined ? "" : ` ${className}`}`}
            onKeyDown={onKeyDown}
        >
            <CodeMirror
                basicSetup={{
                    autocompletion: false,
                    bracketMatching: true,
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
    return "text";
}

function extensionsForLanguage(language: CodeEditorLanguage): Extension[] {
    switch (language) {
        case "json":
            return [json()];
        case "toml":
            return [StreamLanguage.define(toml)];
        case "text":
            return [];
    }
}
