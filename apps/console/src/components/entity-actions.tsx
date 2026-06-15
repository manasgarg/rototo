import { FormEvent, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { useRouter } from "@/lib/navigation";
import { apiFetch } from "@/lib/api";

/** Editable workspace section supported by the add-entity form. */
type EntityKind =
    | "variables"
    | "qualifiers"
    | "catalogs"
    | "schemas"
    | "context"
    | "linters";

/** Transient submit/delete result shown by entity action forms. */
type FormNote = { tone: "ok" | "err"; text: string };

export function AddEntityForm({
    disabled,
    draftId,
    kind,
    workspaceId,
}: {
    disabled?: boolean;
    draftId: string;
    kind: EntityKind;
    workspaceId: string;
}) {
    const router = useRouter();
    const [id, setId] = useState("");
    const [variableType, setVariableType] = useState("string");
    const [pending, setPending] = useState(false);
    const [note, setNote] = useState<FormNote | null>(null);

    async function submit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault();
        setPending(true);
        setNote(null);
        try {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/drafts/${draftId}/entities`,
                {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ kind, id, variableType }),
                },
            );
            const body = (await response.json()) as {
                error?: string;
                files?: Array<{ path: string }>;
            };
            if (!response.ok) {
                throw new Error(body.error ?? "failed to add entity");
            }
            setId("");
            setNote({ tone: "ok", text: "Added to the draft branch." });
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

    return (
        <form className="card" onSubmit={submit}>
            <div className="card-head-text">
                <h3>Add a {kindLabel(kind)}</h3>
                <p className="hint">{addHelp(kind)}</p>
            </div>
            <div className="field-grid">
                <label className="field-stack">
                    <span className="label">id</span>
                    <input
                        className="input mono"
                        disabled={disabled || pending}
                        onChange={(event) => setId(event.target.value)}
                        placeholder={placeholder(kind)}
                        value={id}
                    />
                </label>
                {kind === "variables" ? (
                    <label className="field-stack">
                        <span className="label">primitive type</span>
                        <select
                            className="input mono"
                            disabled={disabled || pending}
                            onChange={(event) =>
                                setVariableType(event.target.value)
                            }
                            value={variableType}
                        >
                            <option value="string">string</option>
                            <option value="bool">bool</option>
                            <option value="int">int</option>
                            <option value="number">number</option>
                            <option value="list">list</option>
                        </select>
                    </label>
                ) : null}
            </div>
            <div className="action-row">
                <button
                    className="btn btn-secondary"
                    disabled={disabled || pending || id.trim() === ""}
                    type="submit"
                >
                    {pending ? (
                        <span className="spin" />
                    ) : (
                        <Plus aria-hidden size={15} />
                    )}
                    {pending ? "Adding" : "Add"}
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

export function AddCatalogEntryForm({
    disabled,
    draftId,
    catalogId,
    workspaceId,
}: {
    disabled?: boolean;
    draftId: string;
    catalogId: string;
    workspaceId: string;
}) {
    const router = useRouter();
    const [id, setId] = useState("");
    const [pending, setPending] = useState(false);
    const [note, setNote] = useState<FormNote | null>(null);

    async function submit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault();
        setPending(true);
        setNote(null);
        try {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/drafts/${draftId}/entities`,
                {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({
                        kind: "catalog_entries",
                        id,
                        catalogId,
                    }),
                },
            );
            const body = (await response.json()) as {
                error?: string;
                files?: Array<{ path: string }>;
            };
            if (!response.ok) {
                throw new Error(body.error ?? "failed to add catalog entry");
            }
            setId("");
            setNote({ tone: "ok", text: "Added to the draft branch." });
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

    return (
        <form className="card" onSubmit={submit}>
            <div className="card-head-text">
                <h3>Add a catalog entry</h3>
                <p className="hint">
                    Creates an entry under{" "}
                    <span className="mono">catalogs/{catalogId}-entries</span>.
                </p>
            </div>
            <label className="field-stack">
                <span className="label">entry key</span>
                <input
                    className="input mono"
                    disabled={disabled || pending}
                    onChange={(event) => setId(event.target.value)}
                    placeholder="new-entry"
                    value={id}
                />
            </label>
            <div className="action-row">
                <button
                    className="btn btn-secondary"
                    disabled={disabled || pending || id.trim() === ""}
                    type="submit"
                >
                    {pending ? (
                        <span className="spin" />
                    ) : (
                        <Plus aria-hidden size={15} />
                    )}
                    {pending ? "Adding" : "Add entry"}
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

export function DeleteEntityButton({
    disabled,
    draftId,
    filePath,
    returnHref,
    workspaceId,
}: {
    disabled?: boolean;
    draftId: string;
    filePath: string;
    returnHref: string;
    workspaceId: string;
}) {
    const router = useRouter();
    const [pending, setPending] = useState(false);
    const [message, setMessage] = useState<string | null>(null);

    async function remove() {
        if (!window.confirm(`Delete ${filePath} from the draft branch?`)) {
            return;
        }
        setPending(true);
        setMessage(null);
        try {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/drafts/${draftId}/files`,
                {
                    method: "DELETE",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ filePath }),
                },
            );
            const body = (await response.json()) as { error?: string };
            if (!response.ok) {
                throw new Error(body.error ?? "failed to delete entity");
            }
            router.push(returnHref);
            router.refresh();
        } catch (error) {
            setMessage(error instanceof Error ? error.message : String(error));
        } finally {
            setPending(false);
        }
    }

    return (
        <div className="action-row">
            {message ? (
                <p className="form-note" data-tone="err">
                    {message}
                </p>
            ) : null}
            <button
                className="btn btn-danger"
                disabled={disabled || pending}
                onClick={remove}
                type="button"
            >
                {pending ? (
                    <span className="spin" />
                ) : (
                    <Trash2 aria-hidden size={15} />
                )}
                {pending ? "Deleting" : "Delete"}
            </button>
        </div>
    );
}

function kindLabel(kind: EntityKind): string {
    if (kind === "context") {
        return "context example";
    }
    return kind.slice(0, -1);
}

function addHelp(kind: EntityKind): string {
    if (kind === "catalogs") {
        return "Creates a catalog file, its schema, and a default catalog entry.";
    }
    if (kind === "context") {
        return "Creates a JSON context example.";
    }
    return "Creates a starter definition on the draft branch.";
}

function placeholder(kind: EntityKind): string {
    if (kind === "schemas") {
        return "example.schema";
    }
    if (kind === "context") {
        return "premium-enterprise";
    }
    return "new-entity";
}
