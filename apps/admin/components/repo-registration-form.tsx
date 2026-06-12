"use client";

import { useRouter } from "next/navigation";
import { FormEvent, useState } from "react";
import { Plus, X } from "lucide-react";

type FormNote = { tone: "ok" | "err"; text: string };

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
      const response = await fetch("/api/repos", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ repo, ref: ref || undefined }),
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
        <button className="btn btn-primary" onClick={() => setOpen(true)} type="button">
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
            rototo scans the ref for <span className="mono">rototo-workspace.toml</span>{" "}
            files. Leave the branch empty to use the repository default.
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
          {pending ? <span className="spin" /> : <Plus aria-hidden size={15} />}
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
