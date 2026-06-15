import { useRouter } from "@/lib/navigation";
import { FormEvent, useState } from "react";
import { Plus, X } from "lucide-react";
import { apiFetch } from "@/lib/api";

/** Transient submit result shown by the repository registration form. */
type FormNote = { tone: "ok" | "err"; text: string };

const REPO_SPEC_ERROR =
    "repository must be owner/repo or a GitHub repository URL";

export function RepoRegistrationForm() {
    const router = useRouter();
    const [open, setOpen] = useState(false);
    const [repo, setRepo] = useState("");
    const [ref, setRef] = useState("");
    const [pending, setPending] = useState(false);
    const [note, setNote] = useState<FormNote | null>(null);

    async function submit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault();
        setPending(true);
        setNote(null);
        try {
            const normalizedRepo = normalizeRepoInput(repo);
            if (!normalizedRepo) {
                throw new Error("repository is required");
            }
            const normalizedRef = ref.trim();
            const response = await apiFetch("/api/repos", {
                method: "POST",
                body: JSON.stringify({
                    repo: normalizedRepo,
                    ref: normalizedRef || undefined,
                }),
            });
            const body = (await response.json()) as { error?: string };
            if (!response.ok) {
                throw new Error(body.error ?? "failed to register repository");
            }
            setRepo("");
            setRef("");
            setNote({ tone: "ok", text: "Repository scanned." });
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
                    Add repository
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
                    <h3>Add a repository</h3>
                    <p className="hint">
                        rototo scans the ref for{" "}
                        <span className="mono">rototo-workspace.toml</span>{" "}
                        files. Leave the branch empty to use the repository
                        default.
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
                    <span className="label">repository</span>
                    <input
                        autoComplete="off"
                        autoFocus
                        className="input mono"
                        onChange={(event) => setRepo(event.target.value)}
                        placeholder="owner/repo"
                        required
                        value={repo}
                    />
                </label>
                <label className="field-stack">
                    <span className="label">branch</span>
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
                    disabled={pending || !repo.trim()}
                    type="submit"
                >
                    {pending ? (
                        <span className="spin" />
                    ) : (
                        <Plus aria-hidden size={15} />
                    )}
                    {pending ? "Scanning" : "Add repository"}
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

function normalizeRepoInput(value: string): string {
    let normalized = value.trim();
    normalized = normalized.replace(/^git@github\.com:/i, "");
    normalized = normalized.replace(/^ssh:\/\/git@github\.com\//i, "");
    normalized = normalized.replace(/^https?:\/\/github\.com\//i, "");
    normalized = normalized.replace(/^github\.com\//i, "");
    normalized = normalized.split(/[?#]/, 1)[0]?.replace(/\/+$/, "") ?? "";
    if (!normalized) {
        return "";
    }
    const [owner, name, ...extra] = normalized.split("/");
    const repoName = name?.replace(/\.git$/i, "");
    const valid = (part: string | undefined) =>
        !!part && /^[A-Za-z0-9_.-]+$/.test(part);
    if (extra.length > 0 || !valid(owner) || !valid(repoName)) {
        throw new Error(REPO_SPEC_ERROR);
    }
    return `${owner}/${repoName}`;
}
