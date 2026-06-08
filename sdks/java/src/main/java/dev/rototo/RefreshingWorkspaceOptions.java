package dev.rototo;

public final class RefreshingWorkspaceOptions {
    private static final RefreshingWorkspaceOptions DEFAULT =
            new RefreshingWorkspaceOptions(null, null, LintMode.DENY);

    private final Double periodSeconds;
    private final String workspaceToken;
    private final LintMode lint;

    private RefreshingWorkspaceOptions(Double periodSeconds, String workspaceToken, LintMode lint) {
        this.periodSeconds = periodSeconds;
        this.workspaceToken = workspaceToken;
        this.lint = lint == null ? LintMode.DENY : lint;
    }

    public static RefreshingWorkspaceOptions defaults() {
        return DEFAULT;
    }

    public static Builder builder() {
        return new Builder();
    }

    public Double periodSeconds() {
        return periodSeconds;
    }

    public String workspaceToken() {
        return workspaceToken;
    }

    public LintMode lint() {
        return lint;
    }

    public static final class Builder {
        private Double periodSeconds;
        private String workspaceToken;
        private LintMode lint = LintMode.DENY;

        private Builder() {}

        public Builder periodSeconds(Double periodSeconds) {
            this.periodSeconds = periodSeconds;
            return this;
        }

        public Builder workspaceToken(String workspaceToken) {
            this.workspaceToken = workspaceToken;
            return this;
        }

        public Builder lint(LintMode lint) {
            this.lint = lint;
            return this;
        }

        public RefreshingWorkspaceOptions build() {
            return new RefreshingWorkspaceOptions(periodSeconds, workspaceToken, lint);
        }
    }
}
