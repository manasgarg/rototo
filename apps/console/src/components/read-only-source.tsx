import {
    CodeEditor,
    type CodeEditorLanguage,
    type CodeEditorMark,
} from "./code-editor";

/* Syntax-highlighted read-only source for inspect screens, with the same
   diagnostic line tints as the branch editor. */
export function ReadOnlySource({
    language,
    marks,
    text,
}: {
    language: CodeEditorLanguage;
    marks: CodeEditorMark[];
    text: string;
}) {
    return (
        <CodeEditor
            disabled
            language={language}
            marks={marks}
            onChange={() => {}}
            value={text}
        />
    );
}
