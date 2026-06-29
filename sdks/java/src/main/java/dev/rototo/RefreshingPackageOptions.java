package dev.rototo;

public final class RefreshingPackageOptions {
    private static final RefreshingPackageOptions DEFAULT =
            new RefreshingPackageOptions(null, null, LintMode.DENY);

    private final Double periodSeconds;
    private final String packageToken;
    private final LintMode lint;

    private RefreshingPackageOptions(Double periodSeconds, String packageToken, LintMode lint) {
        this.periodSeconds = periodSeconds;
        this.packageToken = packageToken;
        this.lint = lint == null ? LintMode.DENY : lint;
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

    public static final class Builder {
        private Double periodSeconds;
        private String packageToken;
        private LintMode lint = LintMode.DENY;

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

        public RefreshingPackageOptions build() {
            return new RefreshingPackageOptions(periodSeconds, packageToken, lint);
        }
    }
}
