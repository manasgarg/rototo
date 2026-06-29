package dev.rototo;

public final class LoadOptions {
    private static final LoadOptions DEFAULT = new LoadOptions(null, LintMode.DENY);

    private final String packageToken;
    private final LintMode lint;

    private LoadOptions(String packageToken, LintMode lint) {
        this.packageToken = packageToken;
        this.lint = lint == null ? LintMode.DENY : lint;
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

    public static final class Builder {
        private String packageToken;
        private LintMode lint = LintMode.DENY;

        private Builder() {}

        public Builder packageToken(String packageToken) {
            this.packageToken = packageToken;
            return this;
        }

        public Builder lint(LintMode lint) {
            this.lint = lint;
            return this;
        }

        public LoadOptions build() {
            return new LoadOptions(packageToken, lint);
        }
    }
}
