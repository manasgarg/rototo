import { useCallback, useEffect, useRef, useState } from "react";

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

export function apiFetch(input: string, init: RequestInit = {}): Promise<Response> {
  return fetch(input, {
    ...init,
    headers: {
      "x-rototo-console": "1",
      ...(init.body !== undefined ? { "content-type": "application/json" } : {}),
      ...init.headers,
    },
  });
}

export async function api<T>(input: string, init: RequestInit = {}): Promise<T> {
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
    active.current = key;
    setLoading(true);
    api<T>(path).then(
      (loaded) => {
        if (active.current === key) {
          setData(loaded);
          setError(null);
          setStatus(200);
          setLoading(false);
        }
      },
      (failure: unknown) => {
        if (active.current === key) {
          setError(failure instanceof Error ? failure.message : String(failure));
          setStatus(failure instanceof ApiError ? failure.status : null);
          setLoading(false);
        }
      },
    );
  }, [path, generation]);

  const reload = useCallback(() => setGeneration((current) => current + 1), []);
  return { data, error, status, loading, reload };
}
