// Wire types for the TypeScript console server (apps/console-server). The
// server is the source of truth for these shapes.

export type Decision = {
    allow: boolean;
    backend: "local" | "grant" | "github" | null;
    reason: string;
};

export type CapabilitySummary = {
    view: Decision;
    propose: Decision;
    approve: Decision;
    administer: Decision;
};

export type SourceTreeSummary = {
    id: string;
    kind: "github" | "local";
    owner: string | null;
    name: string | null;
    defaultBranch: string | null;
    resource: string;
    capabilities: CapabilitySummary;
};

export type MeResponse = {
    authMode: "local" | "team";
    principal: {
        id: string;
        kind: string;
        displayName: string;
        status: string;
    } | null;
    identities: unknown[];
    enrollment: "enrolled" | null;
    githubCredentialSource?: string | null;
    signIn?: { github: boolean };
    capabilities?: {
        deployment: CapabilitySummary;
        sourceTrees: SourceTreeSummary[];
    };
};

export async function fetchMe(): Promise<MeResponse> {
    const response = await fetch("/api/me");
    if (!response.ok) {
        throw new Error(`the console server answered ${response.status}`);
    }
    return (await response.json()) as MeResponse;
}
