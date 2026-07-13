package dev.rototo;

public final class RefreshingPackageOptions {
    private static final RefreshingPackageOptions DEFAULT =
            new RefreshingPackageOptions(null, null, LintMode.DENY, null, null);

    private final Double periodSeconds;
    private final String packageToken;
    private final LintMode lint;
    private final String fallbackSource;
    private final java.util.Map<String, String> packageTokens;

    private RefreshingPackageOptions(
            Double periodSeconds,
            String packageToken,
            LintMode lint,
            String fallbackSource,
            java.util.Map<String, String> packageTokens) {
        this.periodSeconds = periodSeconds;
        this.packageToken = packageToken;
        this.lint = lint == null ? LintMode.DENY : lint;
        this.fallbackSource = fallbackSource;
        this.packageTokens =
                packageTokens == null ? null : java.util.Map.copyOf(packageTokens);
    }

    public static RefreshingPackageOptions defaults() {
        return DEFAULT;
    }

    public static Builder builder() {
        return new Builder();
    }

    public Double periodSeconds() {
        return periodSeconds;
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

    /** Bearer tokens scoped to https:// URL prefixes, or null. */
    public java.util.Map<String, String> packageTokens() {
        return packageTokens;
    }

    public static final class Builder {
        private Double periodSeconds;
        private String packageToken;
        private LintMode lint = LintMode.DENY;
        private String fallbackSource;
        private java.util.Map<String, String> packageTokens;

        private Builder() {}

        public Builder periodSeconds(Double periodSeconds) {
            this.periodSeconds = periodSeconds;
            return this;
        }

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
         */
        public Builder fallbackSource(String fallbackSource) {
            this.fallbackSource = fallbackSource;
            return this;
        }

        /**
         * Bearer tokens scoped to https:// URL prefixes; the longest matching
         * prefix wins. Mutually exclusive with packageToken.
         */
        public Builder packageTokens(java.util.Map<String, String> packageTokens) {
            this.packageTokens = packageTokens;
            return this;
        }

        public RefreshingPackageOptions build() {
            return new RefreshingPackageOptions(
                    periodSeconds, packageToken, lint, fallbackSource, packageTokens);
        }
    }
}
