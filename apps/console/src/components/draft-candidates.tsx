import { useRouter } from "@/lib/navigation";
import { useEffect, useState } from "react";
import { GitBranchPlus, Search } from "lucide-react";
import { apiFetch } from "@/lib/api";

type Candidate = {
    branch: string;
    aheadBy: number;
    filesChanged: number;
};

type ScanState =
    | { kind: "loading" }
    | { kind: "error"; message: string }
    | { kind: "done"; candidates: Candidate[]; skipped: number };

export function DraftCandidates({ workspaceId }: { workspaceId: string }) {
    const router = useRouter();
    const [scan, setScan] = useState<ScanState>({ kind: "loading" });
    const [openingBranch, setOpeningBranch] = useState<string | null>(null);
    const [openError, setOpenError] = useState<string | null>(null);

    useEffect(() => {
        let cancelled = false;
        (async () => {
            try {
                const response = await apiFetch(
                    `/api/workspaces/${workspaceId}/draft-candidates`,
                );
                const body = (await response.json()) as {
                    candidates?: Candidate[];
                    skipped?: number;
                    error?: string;
                };
                if (cancelled) {
                    return;
                }
                if (!response.ok || !body.candidates) {
                    throw new Error(body.error ?? "failed to scan branches");
                }
                setScan({
                    kind: "done",
                    candidates: body.candidates,
                    skipped: body.skipped ?? 0,
                });
            } catch (error) {
                if (!cancelled) {
                    setScan({
                        kind: "error",
                        message:
                            error instanceof Error
                                ? error.message
                                : String(error),
                    });
                }
            }
        })();
        return () => {
            cancelled = true;
        };
    }, [workspaceId]);

    async function openDraft(branch: string) {
        setOpeningBranch(branch);
        setOpenError(null);
        try {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/drafts`,
                {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ branch }),
                },
            );
            const body = (await response.json()) as {
                draft?: { id: string };
                error?: string;
            };
            if (!response.ok || !body.draft) {
                throw new Error(
                    body.error ?? "failed to open the branch as a draft",
                );
            }
            router.push(
                `/app/workspaces/${workspaceId}/drafts/${body.draft.id}`,
            );
        } catch (error) {
            setOpenError(
                error instanceof Error ? error.message : String(error),
            );
            setOpeningBranch(null);
        }
    }

    if (scan.kind === "done" && scan.candidates.length === 0) {
        return null;
    }

    return (
        <div className="card">
            <div className="card-head-text">
                <h3>Branches with workspace changes</h3>
                <p className="hint">
                    Discovered from GitHub: branches whose changes touch only
                    this workspace. Open one to review and publish it from here.
                </p>
            </div>
            {scan.kind === "loading" ? (
                <div className="row-list">
                    <div className="skeleton" style={{ height: 40 }} />
                    <div className="skeleton" style={{ height: 40 }} />
                </div>
            ) : null}
            {scan.kind === "error" ? (
                <p className="form-note" data-tone="err">
                    Branch scan failed: {scan.message}
                </p>
            ) : null}
            {scan.kind === "done" ? (
                <>
                    <div className="row-list">
                        {scan.candidates.map((candidate) => (
                            <div className="row" key={candidate.branch}>
                                <span className="row-icon">
                                    <Search aria-hidden size={15} />
                                </span>
                                <span className="row-text">
                                    <span className="row-title mono">
                                        {candidate.branch}
                                    </span>
                                    <span className="row-sub">
                                        {candidate.filesChanged}{" "}
                                        {candidate.filesChanged === 1
                                            ? "file"
                                            : "files"}{" "}
                                        changed · ahead by {candidate.aheadBy}
                                    </span>
                                </span>
                                <span className="row-side">
                                    <button
                                        className="btn btn-secondary btn-sm"
                                        disabled={openingBranch !== null}
                                        onClick={() =>
                                            openDraft(candidate.branch)
                                        }
                                        type="button"
                                    >
                                        {openingBranch === candidate.branch ? (
                                            <span className="spin" />
                                        ) : (
                                            <GitBranchPlus
                                                aria-hidden
                                                size={14}
                                            />
                                        )}
                                        Open draft
                                    </button>
                                </span>
                            </div>
                        ))}
                    </div>
                    {scan.skipped > 0 ? (
                        <span className="field-hint">
                            {scan.skipped} more{" "}
                            {scan.skipped === 1
                                ? "branch was"
                                : "branches were"}{" "}
                            not scanned; open one by name below if it is missing
                            here.
                        </span>
                    ) : null}
                    {openError ? (
                        <p className="form-note" data-tone="err">
                            {openError}
                        </p>
                    ) : null}
                </>
            ) : null}
        </div>
    );
}
