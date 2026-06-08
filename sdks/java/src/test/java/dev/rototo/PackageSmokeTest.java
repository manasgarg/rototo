package dev.rototo;

import java.util.Map;
import java.util.concurrent.TimeUnit;

public final class PackageSmokeTest {
    public static void main(String[] args) throws Exception {
        if (!Rototo.version().equals("0.1.0-alpha.4")) {
            throw new AssertionError("unexpected version: " + Rototo.version());
        }
        try (Workspace workspace = Workspace.load("examples/basic").get(30, TimeUnit.SECONDS)) {
            VariableResolution resolution = workspace.resolveVariable(
                    "premium-message",
                    Map.of("user", Map.of("tier", "premium"))).get(30, TimeUnit.SECONDS);
            if (!resolution.valueKey().equals("premium")) {
                throw new AssertionError("unexpected package smoke value key: " + resolution.valueKey());
            }
        }
    }
}
