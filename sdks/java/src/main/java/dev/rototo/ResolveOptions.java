package dev.rototo;

public final class ResolveOptions {
    private static final ResolveOptions DEFAULT = new ResolveOptions(true, false, null);

    private final boolean validateContext;
    private final boolean trace;
    private final String tenant;

    private ResolveOptions(boolean validateContext, boolean trace, String tenant) {
        this.validateContext = validateContext;
        this.trace = trace;
        this.tenant = tenant;
    }

    public static ResolveOptions defaults() {
        return DEFAULT;
    }

    public static ResolveOptions validateContext(boolean validateContext) {
        return new ResolveOptions(validateContext, false, null);
    }

    public static ResolveOptions trace(boolean trace) {
        return new ResolveOptions(true, trace, null);
    }

    /**
     * Scope the resolution to one tenant: expressions read the id as
     * {@code env.tenant}. {@code null} means the resolution is not
     * tenant-scoped.
     */
    public static ResolveOptions tenant(String tenant) {
        return new ResolveOptions(true, false, tenant);
    }

    public ResolveOptions withValidateContext(boolean validateContext) {
        return new ResolveOptions(validateContext, this.trace, this.tenant);
    }

    public ResolveOptions withTrace(boolean trace) {
        return new ResolveOptions(this.validateContext, trace, this.tenant);
    }

    public ResolveOptions withTenant(String tenant) {
        return new ResolveOptions(this.validateContext, this.trace, tenant);
    }

    public boolean validateContext() {
        return validateContext;
    }

    public boolean trace() {
        return trace;
    }

    public String tenant() {
        return tenant;
    }
}
