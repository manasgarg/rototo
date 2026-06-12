"use client";

import Link from "next/link";
import { RotateCcw, TriangleAlert } from "lucide-react";

export default function AppError({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  return (
    <main className="fault-page">
      <div className="fault-panel">
        <span className="label">something failed</span>
        <h1>This screen hit an error.</h1>
        <div className="banner banner-err">
          <TriangleAlert aria-hidden size={16} />
          <span>{error.message || "Unknown error."}</span>
        </div>
        <div className="action-row">
          <button className="btn btn-primary" onClick={reset} type="button">
            <RotateCcw aria-hidden size={15} />
            Try again
          </button>
          <Link className="btn btn-secondary" href="/app">
            Back to the console
          </Link>
        </div>
      </div>
    </main>
  );
}
