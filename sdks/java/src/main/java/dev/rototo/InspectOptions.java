package dev.rototo;

public final class InspectOptions {
    private static final InspectOptions DEFAULT = new InspectOptions(null);

    private final String workspaceToken;

    private InspectOptions(String workspaceToken) {
        this.workspaceToken = workspaceToken;
    }

    public static InspectOptions defaults() {
        return DEFAULT;
    }

    public static Builder builder() {
        return new Builder();
    }

    public String workspaceToken() {
        return workspaceToken;
    }

    public static final class Builder {
        private String workspaceToken;

        private Builder() {}

        public Builder workspaceToken(String workspaceToken) {
            this.workspaceToken = workspaceToken;
            return this;
        }

        public InspectOptions build() {
            return new InspectOptions(workspaceToken);
        }
    }
}
