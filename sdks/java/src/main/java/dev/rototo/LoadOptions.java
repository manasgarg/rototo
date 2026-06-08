package dev.rototo;

public final class LoadOptions {
    private static final LoadOptions DEFAULT = new LoadOptions(null, LintMode.DENY);

    private final String workspaceToken;
    private final LintMode lint;

    private LoadOptions(String workspaceToken, LintMode lint) {
        this.workspaceToken = workspaceToken;
        this.lint = lint == null ? LintMode.DENY : lint;
    }

    public static LoadOptions defaults() {
        return DEFAULT;
    }

    public static Builder builder() {
        return new Builder();
    }

    public String workspaceToken() {
        return workspaceToken;
    }

    public LintMode lint() {
        return lint;
    }

    public static final class Builder {
        private String workspaceToken;
        private LintMode lint = LintMode.DENY;

        private Builder() {}

        public Builder workspaceToken(String workspaceToken) {
            this.workspaceToken = workspaceToken;
            return this;
        }

        public Builder lint(LintMode lint) {
            this.lint = lint;
            return this;
        }

        public LoadOptions build() {
            return new LoadOptions(workspaceToken, lint);
        }
    }
}
