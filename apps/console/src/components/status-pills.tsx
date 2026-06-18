/** Minimal branch state needed to render status pills across screens. */
type BranchLike = {
    status: string;
    prState: string | null;
};

export function BranchStatusPill({ branch }: { branch: BranchLike }) {
    if (branch.status === "archived") {
        return <Pill label="archived" tone="neutral" />;
    }
    if (branch.prState) {
        const state = branch.prState;
        if (state === "merged") {
            return <Pill label="merged" tone="ok" />;
        }
        if (state === "closed") {
            return <Pill label="pr closed" tone="err" />;
        }
        if (state === "open") {
            return <Pill label="pr open" tone="info" />;
        }
        return <Pill label={state} tone="info" />;
    }
    if (branch.status === "recent") {
        return <Pill label="recent" tone="neutral" />;
    }
    return <Pill label="active" tone="sea" />;
}

export function Pill({
    label,
    tone,
}: {
    label: string;
    tone: "ok" | "warn" | "err" | "info" | "neutral" | "sea";
}) {
    return (
        <span className={`pill pill-${tone}`}>
            <span className="d" />
            {label}
        </span>
    );
}
