// A hash router small enough to read in one sitting. Routes are hash paths
// ("#/trees/st_x/changes"); deep-linking finer state (selected entity,
// change set) waits until the workbench settles.

import { useEffect, useState } from "react";

export function useHashPath(): string {
    const [hash, setHash] = useState(current);
    useEffect(() => {
        const onChange = () => setHash(current());
        window.addEventListener("hashchange", onChange);
        return () => window.removeEventListener("hashchange", onChange);
    }, []);
    return hash;
}

export function navigate(to: string): void {
    window.location.hash = to;
}

function current(): string {
    const hash = window.location.hash.replace(/^#/, "");
    return hash === "" ? "/" : hash;
}
