package dev.rototo;

import java.util.Map;
import java.util.concurrent.TimeUnit;

public final class PackageSmokeTest {
    public static void main(String[] args) throws Exception {
        String expectedVersion = System.getProperty("rototo.expected.version");
        if (expectedVersion == null || expectedVersion.isBlank()) {
            throw new AssertionError("missing rototo.expected.version");
        }
        if (!Rototo.version().equals(expectedVersion)) {
            throw new AssertionError("expected version " + expectedVersion + ", got " + Rototo.version());
        }
        try (Package pkg = Package.load("examples/basic").get(30, TimeUnit.SECONDS)) {
            VariableResolution resolution = pkg.resolveVariable(
                    "premium_message",
                    Map.of("user", Map.of("tier", "premium")));
            if (!resolution.value().equals("Welcome back, premium member.")
                    || !resolution.source().equals(Map.of("kind", "literal"))) {
                throw new AssertionError("unexpected package smoke resolution: " + resolution.value());
            }
        }
    }
}
