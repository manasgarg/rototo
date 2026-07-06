// A user-visible refusal with an HTTP status. Services throw these; routes
// render them as `{ error: { message, paths? } }` with the status.

export class ApiError extends Error {
    readonly status: number;
    // The overlapping paths of a "changed under you" rejection, when the
    // refusal is the expected-pin staleness check.
    readonly conflictPaths: string[] | undefined;

    constructor(status: number, message: string, conflictPaths?: string[]) {
        super(message);
        this.status = status;
        this.conflictPaths = conflictPaths;
    }
}
