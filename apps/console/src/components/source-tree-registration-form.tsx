import { useRouter } from "@/lib/navigation";
import { FormEvent, useState } from "react";
import { Plus, X } from "lucide-react";
import { apiFetch } from "@/lib/api";

/** Transient submit result shown by the configuration source registration form. */
type FormNote = { tone: "ok" | "err"; text: string };

export function SourceTreeRegistrationForm() {
    const router = useRouter();
    const [open, setOpen] = useState(false);
    const [sourceTree, setSourceTree] = useState("");
    const [ref, setRef] = useState("");
    const [pending, setPending] = useState(false);
    const [note, setNote] = useState<FormNote | null>(null);

    async function submit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault();
        setPending(true);
        setNote(null);
        try {
            const normalizedSourceTree = normalizeSourceTreeInput(sourceTree);
            if (!normalizedSourceTree) {
                throw new Error("configuration source is required");
            }
            const normalizedRef = ref.trim();
            const response = await apiFetch("/api/source-trees", {
                method: "POST",
                body: JSON.stringify({
                    sourceTree: normalizedSourceTree,
                    ref: normalizedRef || undefined,
                }),
            });
            const body = (await response.json()) as { error?: string };
            if (!response.ok) {
                throw new Error(
                    body.error ?? "failed to register configuration source",
                );
            }
            setSourceTree("");
            setRef("");
            setNote({ tone: "ok", text: "Configuration source scanned." });
            setOpen(false);
            router.refresh();
        } catch (error) {
            setNote({
                tone: "err",
                text: error instanceof Error ? error.message : String(error),
            });
        } finally {
            setPending(false);
        }
    }

    if (!open) {
        return (
            <div className="action-row">
                <button
                    className="btn btn-primary"
                    onClick={() => setOpen(true)}
                    type="button"
                >
                    <Plus aria-hidden size={15} />
                    Add configuration source
                </button>
                {note ? (
                    <p className="form-note" data-tone={note.tone}>
                        {note.text}
                    </p>
                ) : null}
            </div>
        );
    }

    return (
        <form className="card" onSubmit={submit}>
            <div className="card-head">
                <div className="card-head-text">
                    <h3>Add a configuration source</h3>
                    <p className="hint">
                        rototo scans the ref for{" "}
                        <span className="mono">rototo-package.toml</span> files.
                        For git sources, leave the branch empty to use the
                        source default.
                    </p>
                </div>
                <button
                    className="btn btn-ghost btn-icon"
                    onClick={() => setOpen(false)}
                    title="Close"
                    type="button"
                >
                    <X aria-hidden size={15} />
                </button>
            </div>
            <div className="field-row">
                <label className="field-stack">
                    <span className="label">configuration source</span>
                    <input
                        autoComplete="off"
                        autoFocus
                        className="input mono"
                        onChange={(event) => setSourceTree(event.target.value)}
                        placeholder="owner/repo, local path, git+ URL, or archive URL"
                        required
                        value={sourceTree}
                    />
                </label>
                <label className="field-stack">
                    <span className="label">branch/ref</span>
                    <input
                        autoComplete="off"
                        className="input mono"
                        onChange={(event) => setRef(event.target.value)}
                        placeholder="default"
                        value={ref}
                    />
                </label>
            </div>
            <div className="action-row">
                <button
                    className="btn btn-primary"
                    disabled={pending || !sourceTree.trim()}
                    type="submit"
                >
                    {pending ? (
                        <span className="spin" />
                    ) : (
                        <Plus aria-hidden size={15} />
                    )}
                    {pending ? "Scanning" : "Add configuration source"}
                </button>
                {note ? (
                    <p className="form-note" data-tone={note.tone}>
                        {note.text}
                    </p>
                ) : null}
            </div>
        </form>
    );
}

function normalizeSourceTreeInput(value: string): string {
    return value.trim();
}
