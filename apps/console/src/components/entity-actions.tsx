import { FormEvent, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { useRouter } from "@/lib/navigation";
import { apiFetch } from "@/lib/api";

type EntityKind = "variables" | "qualifiers" | "resources" | "schemas" | "context" | "linters";

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
      const response = await apiFetch(`/api/workspaces/${workspaceId}/drafts/${draftId}/entities`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind, id, variableType }),
      });
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
              onChange={(event) => setVariableType(event.target.value)}
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
          {pending ? <span className="spin" /> : <Plus aria-hidden size={15} />}
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

export function AddResourceObjectForm({
  disabled,
  draftId,
  resourceId,
  workspaceId,
}: {
  disabled?: boolean;
  draftId: string;
  resourceId: string;
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
      const response = await apiFetch(`/api/workspaces/${workspaceId}/drafts/${draftId}/entities`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind: "resource_objects", id, resourceId }),
      });
      const body = (await response.json()) as {
        error?: string;
        files?: Array<{ path: string }>;
      };
      if (!response.ok) {
        throw new Error(body.error ?? "failed to add resource object");
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
        <h3>Add a resource object</h3>
        <p className="hint">
          Creates an object under{" "}
          <span className="mono">resources/{resourceId}-objects</span>.
        </p>
      </div>
      <label className="field-stack">
        <span className="label">object key</span>
        <input
          className="input mono"
          disabled={disabled || pending}
          onChange={(event) => setId(event.target.value)}
          placeholder="new-object"
          value={id}
        />
      </label>
      <div className="action-row">
        <button
          className="btn btn-secondary"
          disabled={disabled || pending || id.trim() === ""}
          type="submit"
        >
          {pending ? <span className="spin" /> : <Plus aria-hidden size={15} />}
          {pending ? "Adding" : "Add object"}
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
      const response = await apiFetch(`/api/workspaces/${workspaceId}/drafts/${draftId}/files`, {
        method: "DELETE",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ filePath }),
      });
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
        {pending ? <span className="spin" /> : <Trash2 aria-hidden size={15} />}
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
  if (kind === "resources") {
    return "Creates a resource file, its schema, and a default resource object.";
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
