package dev.rototo;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;

public final class JavaSdkTest {
    public static void main(String[] args) throws Exception {
        api();
        contract();
        refresh();
    }

    private static void api() throws Exception {
        assertEquals(expectedVersion(), Rototo.version(), "version");
        try (Package pkg = await(Package.load("examples/basic"))) {
            VariableResolution variable = await(pkg.resolveVariable(
                    "premium-message",
                    Map.of("user", Map.of("tier", "premium"))));
            assertEquals("premium-message", variable.id(), "variable id");
            assertEquals(Map.of("kind", "literal"), variable.source(), "source");
            assertEquals("Welcome back, premium member.", variable.value(), "value");

            Boolean qualifier = await(pkg.resolveQualifier(
                    "premium-users",
                    Map.of("user", Map.of("tier", "free"))));
            assertEquals(false, qualifier, "qualifier value");

            VariableResolution skippedValidation = await(pkg.resolveVariable(
                    "premium-message",
                    Map.of("user", Map.of("tier", Map.of("bad", "shape"))),
                    ResolveOptions.validateContext(false)));
            assertEquals(Map.of("kind", "literal"), skippedValidation.source(), "validation skip fallback");
        }

        try (Package inspected = await(Package.inspect("examples/basic"))) {
            PackageLint lint = await(inspected.lint());
            assertEquals(0, lint.diagnostics().size(), "inspection lint diagnostics");
            assertRototoError(
                    inspected.resolveVariable("premium-message", Map.of()),
                    "package was loaded without a runtime model");
        }
    }

    private static void contract() throws Exception {
        List<String> lines = Files.readAllLines(Path.of("tests/sdk-contract/cases.jsonl"));
        for (String line : lines) {
            if (line.isBlank()) {
                continue;
            }
            Map<String, Object> testCase = Json.asObject(Json.parse(line));
            String name = Json.asString(testCase.get("name"));
            String operation = Json.asString(testCase.get("operation"));
            String packageSource = Json.asString(testCase.get("package"));
            Map<String, Object> expect = Json.asObject(testCase.get("expect"));
            boolean ok = Json.asBoolean(expect.get("ok"));

            if (operation.equals("load_package")) {
                CompletableFuture<Package> future = Package.load(packageSource);
                if (ok) {
                    await(future).close();
                } else {
                    assertRototoError(future, expectedError(expect));
                }
                continue;
            }

            try (Package pkg = await(Package.load(packageSource))) {
                switch (operation) {
                    case "lint_package":
                        if (ok) {
                            PackageLint lint = await(pkg.lint());
                            assertEquals(
                                    Json.asLong(expect.get("diagnostics")),
                                    (long) lint.diagnostics().size(),
                                    name + " diagnostics");
                        } else {
                            assertRototoError(pkg.lint(), expectedError(expect));
                        }
                        break;
                    case "resolve_qualifier":
                        runQualifierCase(name, pkg, testCase, expect, ok);
                        break;
                    case "resolve_variable":
                        runVariableCase(name, pkg, testCase, expect, ok);
                        break;
                    default:
                        throw new AssertionError("unsupported contract operation: " + operation);
                }
            }
        }
    }

    private static void runQualifierCase(
            String name,
            Package pkg,
            Map<String, Object> testCase,
            Map<String, Object> expect,
            boolean ok) throws Exception {
        CompletableFuture<Boolean> future = pkg.resolveQualifier(
                Json.asString(testCase.get("id")),
                Json.asObject(testCase.get("context")));
        if (!ok) {
            assertRototoError(future, expectedError(expect));
            return;
        }
        Boolean actual = await(future);
        assertEquals(Json.asBoolean(expect.get("result")), actual, name + " value");
    }

    private static void runVariableCase(
            String name,
            Package pkg,
            Map<String, Object> testCase,
            Map<String, Object> expect,
            boolean ok) throws Exception {
        CompletableFuture<VariableResolution> future = pkg.resolveVariable(
                Json.asString(testCase.get("id")),
                Json.asObject(testCase.get("context")));
        if (!ok) {
            assertRototoError(future, expectedError(expect));
            return;
        }
        Map<String, Object> result = Json.asObject(expect.get("result"));
        VariableResolution actual = await(future);
        assertEquals(Json.asString(result.get("id")), actual.id(), name + " id");
        assertEquals(result.get("value"), actual.value(), name + " value");
        assertEquals(result.get("source"), actual.source(), name + " source");
    }

    private static void refresh() throws Exception {
        RefreshingPackageOptions options = RefreshingPackageOptions.builder()
                .periodSeconds(30.0)
                .build();
        try (RefreshingPackage pkg = await(RefreshingPackage.load("examples/basic", options))) {
            VariableResolution resolution = await(pkg.resolveVariable(
                    "premium-message",
                    Map.of("user", Map.of("tier", "premium"))));
            assertEquals(Map.of("kind", "literal"), resolution.source(), "refreshing resolution");
            RefreshStatus status = await(pkg.status());
            assertEquals(0L, status.consecutiveFailures(), "consecutive failures");
            assertEquals(false, status.refreshing(), "refreshing flag");
            await(pkg.shutdown());
        }
    }

    private static String expectedError(Map<String, Object> expect) {
        return Json.asString(Json.asObject(expect.get("error")).get("contains"));
    }

    private static <T> T await(CompletableFuture<T> future) throws Exception {
        return future.get(30, TimeUnit.SECONDS);
    }

    private static void assertRototoError(CompletableFuture<?> future, String contains) throws Exception {
        try {
            await(future);
        } catch (ExecutionException error) {
            Throwable cause = error.getCause();
            if (!(cause instanceof RototoException)) {
                throw new AssertionError("expected RototoException, got " + cause, cause);
            }
            if (!cause.getMessage().contains(contains)) {
                throw new AssertionError(
                        "expected error containing " + contains + ", got " + cause.getMessage());
            }
            return;
        }
        throw new AssertionError("expected RototoException containing " + contains);
    }

    private static void assertEquals(Object expected, Object actual, String label) {
        if (!expected.equals(actual)) {
            throw new AssertionError(label + ": expected " + expected + ", got " + actual);
        }
    }

    private static String expectedVersion() {
        String version = System.getProperty("rototo.expected.version");
        if (version == null || version.isBlank()) {
            throw new AssertionError("missing rototo.expected.version");
        }
        return version;
    }
}
