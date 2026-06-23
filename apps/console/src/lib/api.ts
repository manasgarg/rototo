import { useCallback, useEffect, useRef, useState } from "react";

import { describeError, recordConsoleEvent } from "./observability";

/* All console API calls go through here so every request carries the
   x-rototo-console header — the server rejects mutations without it, which
   is what makes cross-site request forgery a non-issue. */

export class ApiError extends Error {
    constructor(
        public readonly status: number,
        message: string,
    ) {
        super(message);
        this.name = "ApiError";
    }
}

export function apiFetch(
    input: string,
    init: RequestInit = {},
): Promise<Response> {
    const started = performance.now();
    const path = input;
    const headers = new Headers(init.headers);
    headers.set("x-rototo-console", "1");
    if (init.body !== undefined && !headers.has("content-type")) {
        headers.set("content-type", "application/json");
    }
    return fetch(input, {
        ...init,
        headers,
    }).then(
        (response) => {
            if (!path.startsWith("/api/dev/observability/")) {
                recordConsoleEvent({
                    kind: "api-fetch",
                    method: init.method ?? "GET",
                    path,
                    status: response.status,
                    ok: response.ok,
                    latencyMs: Math.round(performance.now() - started),
                });
            }
            return response;
        },
        (error: unknown) => {
            if (!path.startsWith("/api/dev/observability/")) {
                recordConsoleEvent({
                    kind: "api-fetch",
                    method: init.method ?? "GET",
                    path,
                    ok: false,
                    latencyMs: Math.round(performance.now() - started),
                    error: describeError(error),
                });
            }
            throw error;
        },
    );
}

export async function api<T>(
    input: string,
    init: RequestInit = {},
): Promise<T> {
    const response = await apiFetch(input, init);
    if (!response.ok) {
        let message = `${response.status} ${response.statusText}`;
        try {
            const body = (await response.json()) as { error?: unknown };
            if (typeof body.error === "string" && body.error) {
                message = body.error;
            }
        } catch {
            // not JSON; keep the status line
        }
        throw new ApiError(response.status, message);
    }
    return (await response.json()) as T;
}

/** Screen-level API request state owned by `useApi` for one endpoint path. */
export type ApiState<T> = {
    data: T | null;
    error: string | null;
    status: number | null;
    loading: boolean;
    reload: () => void;
};

/* One screen-level data fetch: load on mount and whenever the path changes,
   expose reload for after mutations. Pass null to skip fetching. */
export function useApi<T>(path: string | null): ApiState<T> {
    const [data, setData] = useState<T | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [status, setStatus] = useState<number | null>(null);
    const [loading, setLoading] = useState(path !== null);
    const [generation, setGeneration] = useState(0);
    const active = useRef<string | null>(null);

    useEffect(() => {
        if (path === null) {
            active.current = null;
            setData(null);
            setError(null);
            setStatus(null);
            setLoading(false);
            return;
        }
        const key = `${path}#${generation}`;
        const started = performance.now();
        active.current = key;
        setLoading(true);
        api<T>(path).then(
            (loaded) => {
                if (active.current === key) {
                    setData(loaded);
                    setError(null);
                    setStatus(200);
                    setLoading(false);
                    recordConsoleEvent({
                        kind: "route-load",
                        path,
                        ok: true,
                        latencyMs: Math.round(performance.now() - started),
                    });
                }
            },
            (failure: unknown) => {
                if (active.current === key) {
                    setError(
                        failure instanceof Error
                            ? failure.message
                            : String(failure),
                    );
                    setStatus(
                        failure instanceof ApiError ? failure.status : null,
                    );
                    setLoading(false);
                    recordConsoleEvent({
                        kind: "route-load",
                        path,
                        ok: false,
                        status:
                            failure instanceof ApiError ? failure.status : null,
                        latencyMs: Math.round(performance.now() - started),
                        error: describeError(failure),
                    });
                }
            },
        );
    }, [path, generation]);

    const reload = useCallback(
        () => setGeneration((current) => current + 1),
        [],
    );
    return { data, error, status, loading, reload };
}
