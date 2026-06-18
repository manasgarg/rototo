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

/* Session identity for the whole app. /api/me returns deployment metadata even
   when hosted deployment is signed out, so both 200 and 401 land here as a
   MeResponse. */

/** React context state for the current `/api/me` payload and reload hook. */
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
                // 401 still carries deployment metadata for signed-out hosted users.
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
        githubLogin: me?.user?.displayName ?? "…",
        githubAvatarUrl: me?.user?.avatarUrl ?? null,
    };
}
