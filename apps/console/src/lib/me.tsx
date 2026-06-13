import {
    type ReactNode,
    createContext,
    useCallback,
    useContext,
    useEffect,
    useState,
} from "react";

import { apiFetch } from "./api";
import type { MeResponse } from "./types";

/* Session identity for the whole app. /api/me returns 200 with mode metadata
   in local and read-only modes even when no user is connected; team mode
   returns 401 when signed out — both land here as a MeResponse. */

type MeState = {
    me: MeResponse | null;
    error: string | null;
    loading: boolean;
    reload: () => void;
};

const MeContext = createContext<MeState>({
    me: null,
    error: null,
    loading: true,
    reload: () => {},
});

export function MeProvider({ children }: { children: ReactNode }) {
    const [me, setMe] = useState<MeResponse | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [loading, setLoading] = useState(true);
    const [generation, setGeneration] = useState(0);

    useEffect(() => {
        let cancelled = false;
        setLoading(true);
        apiFetch("/api/me")
            .then(async (response) => {
                // 401 still carries the mode payload (team mode, signed out).
                const body = (await response.json()) as MeResponse;
                if (!cancelled) {
                    setMe(body);
                    setError(null);
                    setLoading(false);
                }
            })
            .catch((failure: unknown) => {
                if (!cancelled) {
                    setError(
                        failure instanceof Error
                            ? failure.message
                            : String(failure),
                    );
                    setLoading(false);
                }
            });
        return () => {
            cancelled = true;
        };
    }, [generation]);

    const reload = useCallback(
        () => setGeneration((current) => current + 1),
        [],
    );
    return (
        <MeContext.Provider value={{ me, error, loading, reload }}>
            {children}
        </MeContext.Provider>
    );
}

export function useMe(): MeState {
    return useContext(MeContext);
}

/* The signed-in user for shell chrome. Screens render inside RequireAuth, so
   a missing user only happens transiently. */
export function useShellUser(): {
    githubLogin: string;
    githubAvatarUrl: string | null;
} {
    const { me } = useMe();
    return {
        githubLogin: me?.user?.githubLogin ?? "…",
        githubAvatarUrl: me?.user?.githubAvatarUrl ?? null,
    };
}
