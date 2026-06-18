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
        assertEquals("0.1.0-alpha.4", Rototo.version(), "version");
        try (Workspace workspace = await(Workspace.load("examples/basic"))) {
            VariableResolution variable = await(workspace.resolveVariable(
                    "premium-message",
                    Map.of("user", Map.of("tier", "premium"))));
            assertEquals("premium-message", variable.id(), "variable id");
            assertEquals(Map.of("kind", "literal"), variable.source(), "source");
            assertEquals("Welcome back, premium member.", variable.value(), "value");

            QualifierResolution qualifier = await(workspace.resolveQualifier(
                    "premium-users",
                    Map.of("user", Map.of("tier", "free"))));
            assertEquals("premium-users", qualifier.id(), "qualifier id");
            assertEquals(false, qualifier.value(), "qualifier value");

            VariableResolution skippedValidation = await(workspace.resolveVariable(
                    "premium-message",
                    Map.of("user", Map.of("tier", Map.of("bad", "shape"))),
                    ResolveOptions.validateContext(false)));
            assertEquals(Map.of("kind", "literal"), skippedValidation.source(), "validation skip fallback");
        }

        try (Workspace inspected = await(Workspace.inspect("examples/basic"))) {
            WorkspaceLint lint = await(inspected.lint());
            assertEquals(0, lint.diagnostics().size(), "inspection lint diagnostics");
            assertRototoError(
                    inspected.resolveVariable("premium-message", Map.of()),
                    "workspace was loaded without a runtime model");
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
            String workspaceSource = Json.asString(testCase.get("workspace"));
            Map<String, Object> expect = Json.asObject(testCase.get("expect"));
            boolean ok = Json.asBoolean(expect.get("ok"));

            if (operation.equals("load_workspace")) {
                CompletableFuture<Workspace> future = Workspace.load(workspaceSource);
                if (ok) {
                    await(future).close();
                } else {
                    assertRototoError(future, expectedError(expect));
                }
                continue;
            }

            try (Workspace workspace = await(Workspace.load(workspaceSource))) {
                switch (operation) {
                    case "lint_workspace":
                        if (ok) {
                            WorkspaceLint lint = await(workspace.lint());
                            assertEquals(
                                    Json.asLong(expect.get("diagnostics")),
                                    (long) lint.diagnostics().size(),
                                    name + " diagnostics");
                        } else {
                            assertRototoError(workspace.lint(), expectedError(expect));
                        }
                        break;
                    case "resolve_qualifier":
                        runQualifierCase(name, workspace, testCase, expect, ok);
                        break;
                    case "resolve_variable":
                        runVariableCase(name, workspace, testCase, expect, ok);
                        break;
                    default:
                        throw new AssertionError("unsupported contract operation: " + operation);
                }
            }
        }
    }

    private static void runQualifierCase(
            String name,
            Workspace workspace,
            Map<String, Object> testCase,
            Map<String, Object> expect,
            boolean ok) throws Exception {
        CompletableFuture<QualifierResolution> future = workspace.resolveQualifier(
                Json.asString(testCase.get("id")),
                Json.asObject(testCase.get("context")));
        if (!ok) {
            assertRototoError(future, expectedError(expect));
            return;
        }
        Map<String, Object> result = Json.asObject(expect.get("result"));
        QualifierResolution actual = await(future);
        assertEquals(Json.asString(result.get("id")), actual.id(), name + " id");
        assertEquals(Json.asBoolean(result.get("value")), actual.value(), name + " value");
    }

    private static void runVariableCase(
            String name,
            Workspace workspace,
            Map<String, Object> testCase,
            Map<String, Object> expect,
            boolean ok) throws Exception {
        CompletableFuture<VariableResolution> future = workspace.resolveVariable(
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
        RefreshingWorkspaceOptions options = RefreshingWorkspaceOptions.builder()
                .periodSeconds(30.0)
                .build();
        try (RefreshingWorkspace workspace = await(RefreshingWorkspace.load("examples/basic", options))) {
            VariableResolution resolution = await(workspace.resolveVariable(
                    "premium-message",
                    Map.of("user", Map.of("tier", "premium"))));
            assertEquals(Map.of("kind", "literal"), resolution.source(), "refreshing resolution");
            RefreshStatus status = await(workspace.status());
            assertEquals(0L, status.consecutiveFailures(), "consecutive failures");
            assertEquals(false, status.refreshing(), "refreshing flag");
            await(workspace.shutdown());
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
}
