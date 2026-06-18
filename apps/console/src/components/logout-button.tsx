import { LogOut } from "lucide-react";
import { useState } from "react";

import { api } from "@/lib/api";
import { useMe } from "@/lib/me";

export function LogoutButton() {
    const { me } = useMe();
    const [pending, setPending] = useState(false);
    const [error, setError] = useState<string | null>(null);

    if (me?.deployment !== "hosted") {
        return null;
    }

    const logout = async () => {
        setPending(true);
        setError(null);
        try {
            await api<{ ok: boolean }>("/api/auth/logout", {
                method: "POST",
                body: "{}",
            });
            // Full reload so every in-memory session state resets.
            window.location.assign("/login");
        } catch (failure) {
            setError(
                failure instanceof Error ? failure.message : String(failure),
            );
            setPending(false);
        }
    };
    return (
        <>
            <button
                aria-label={error ? `Sign out failed: ${error}` : "Sign out"}
                className="btn btn-ghost btn-icon"
                disabled={pending}
                onClick={logout}
                title={error ?? "Sign out"}
                type="button"
            >
                {pending ? (
                    <span className="spin" />
                ) : (
                    <LogOut aria-hidden size={15} />
                )}
            </button>
            {error ? (
                <span className="logout-error" role="status">
                    Sign out failed
                </span>
            ) : null}
        </>
    );
}
