import { Link } from "@/lib/link";
import { ArrowLeft } from "lucide-react";
import { RototoMark } from "@/components/rototo-mark";

export function NotFound() {
    return (
        <main className="fault-page">
            <div className="fault-panel">
                <span className="brand-mark">
                    <RototoMark size={32} />
                </span>
                <span className="label">404 — not found</span>
                <h1>Nothing is configured at this address.</h1>
                <p className="hint">
                    The workspace or branch may have been removed, or the link
                    may be stale.
                </p>
                <Link className="btn btn-secondary" href="/app">
                    <ArrowLeft aria-hidden size={15} />
                    Back to the console
                </Link>
            </div>
        </main>
    );
}
