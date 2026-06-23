package dev.rototo;

public final class InspectOptions {
    private static final InspectOptions DEFAULT = new InspectOptions(null);

    private final String packageToken;

    private InspectOptions(String packageToken) {
        this.packageToken = packageToken;
    }

    public static InspectOptions defaults() {
        return DEFAULT;
    }

    public static Builder builder() {
        return new Builder();
    }

    public String packageToken() {
        return packageToken;
    }

    public static final class Builder {
        private String packageToken;

        private Builder() {}

        public Builder packageToken(String packageToken) {
            this.packageToken = packageToken;
            return this;
        }

        public InspectOptions build() {
            return new InspectOptions(packageToken);
        }
    }
}
