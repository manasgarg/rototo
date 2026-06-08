package dev.rototo;

public final class ResolveOptions {
    private static final ResolveOptions DEFAULT = new ResolveOptions(true);

    private final boolean validateContext;

    private ResolveOptions(boolean validateContext) {
        this.validateContext = validateContext;
    }

    public static ResolveOptions defaults() {
        return DEFAULT;
    }

    public static ResolveOptions validateContext(boolean validateContext) {
        return new ResolveOptions(validateContext);
    }

    public boolean validateContext() {
        return validateContext;
    }
}
