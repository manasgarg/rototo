package dev.rototo;

public final class LoadOptions {
    private static final LoadOptions DEFAULT = new LoadOptions(null, LintMode.DENY, null);

    private final String packageToken;
    private final LintMode lint;
    private final String fallbackSource;

    private LoadOptions(String packageToken, LintMode lint, String fallbackSource) {
        this.packageToken = packageToken;
        this.lint = lint == null ? LintMode.DENY : lint;
        this.fallbackSource = fallbackSource;
    }

    public static LoadOptions defaults() {
        return DEFAULT;
    }

    public static Builder builder() {
        return new Builder();
    }

    public String packageToken() {
        return packageToken;
    }

    public LintMode lint() {
        return lint;
    }

    /** Fallback package source for degraded starts, or null. */
    public String fallbackSource() {
        return fallbackSource;
    }

    public static final class Builder {
        private String packageToken;
        private LintMode lint = LintMode.DENY;
        private String fallbackSource;

        private Builder() {}

        public Builder packageToken(String packageToken) {
            this.packageToken = packageToken;
            return this;
        }

        public Builder lint(LintMode lint) {
            this.lint = lint;
            return this;
        }

        /**
         * Names a fallback package source for degraded starts: loaded through
         * the same pipeline when the primary source fails for any reason.
         * Typically a local path to a bundled, app-tested copy of the package.
         */
        public Builder fallbackSource(String fallbackSource) {
            this.fallbackSource = fallbackSource;
            return this;
        }

        public LoadOptions build() {
            return new LoadOptions(packageToken, lint, fallbackSource);
        }
    }
}
