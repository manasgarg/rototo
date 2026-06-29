package dev.rototo;

import java.util.Map;

/** Identity of the SDK that emitted a refresh event. */
public final class SdkIdentity {
    private final String name;
    private final String version;
    private final String language;

    public SdkIdentity(String name, String version, String language) {
        this.name = name;
        this.version = version;
        this.language = language;
    }

    static SdkIdentity fromJson(Map<String, Object> value) {
        return new SdkIdentity(
                Json.asString(value.get("name")),
                Json.asString(value.get("version")),
                Json.asString(value.get("language")));
    }

    public String name() {
        return name;
    }

    public String version() {
        return version;
    }

    public String language() {
        return language;
    }
}
