package dev.rototo;

public final class ResolveOptions {
    private static final ResolveOptions DEFAULT = new ResolveOptions(true, false);

    private final boolean validateContext;
    private final boolean trace;

    private ResolveOptions(boolean validateContext, boolean trace) {
        this.validateContext = validateContext;
        this.trace = trace;
    }

    public static ResolveOptions defaults() {
        return DEFAULT;
    }

    public static ResolveOptions validateContext(boolean validateContext) {
        return new ResolveOptions(validateContext, false);
    }

    public static ResolveOptions trace(boolean trace) {
        return new ResolveOptions(true, trace);
    }

    public ResolveOptions withValidateContext(boolean validateContext) {
        return new ResolveOptions(validateContext, this.trace);
    }

    public ResolveOptions withTrace(boolean trace) {
        return new ResolveOptions(this.validateContext, trace);
    }

    public boolean validateContext() {
        return validateContext;
    }

    public boolean trace() {
        return trace;
    }
}
