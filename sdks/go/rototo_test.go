package rototo

import (
	"bufio"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestWorkspaceExposesGoResolutionObjects(t *testing.T) {
	workspace := loadBasicWorkspace(t)
	defer closeWorkspace(t, workspace)

	variable, err := workspace.ResolveVariable(
		context.Background(),
		"premium-message",
		map[string]any{"user": map[string]any{"tier": "premium"}},
		nil,
	)
	if err != nil {
		t.Fatal(err)
	}
	qualifier, err := workspace.ResolveQualifier(
		context.Background(),
		"premium-users",
		map[string]any{"user": map[string]any{"tier": "premium"}},
		nil,
	)
	if err != nil {
		t.Fatal(err)
	}

	if variable.ID != "premium-message" {
		t.Fatalf("variable id = %q", variable.ID)
	}
	if variable.ValueKey != "premium" {
		t.Fatalf("value key = %q", variable.ValueKey)
	}
	if variable.Value != "Welcome back, premium member." {
		t.Fatalf("value = %#v", variable.Value)
	}
	if qualifier.ID != "premium-users" {
		t.Fatalf("qualifier id = %q", qualifier.ID)
	}
	if !qualifier.Value {
		t.Fatalf("qualifier value = false")
	}
}

func TestInspectedWorkspaceCanLintButNotResolve(t *testing.T) {
	workspace, err := Inspect(context.Background(), basicWorkspace(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer closeWorkspace(t, workspace)

	lint, err := workspace.Lint(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(lint.Diagnostics) != 0 {
		t.Fatalf("diagnostics = %#v", lint.Diagnostics)
	}

	_, err = workspace.ResolveVariable(context.Background(), "premium-message", map[string]any{}, nil)
	if err == nil {
		t.Fatal("expected inspected workspace resolution to fail")
	}
	if !strings.Contains(err.Error(), "workspace was loaded without a runtime model") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestContextValidationCanBeSkipped(t *testing.T) {
	workspace := loadBasicWorkspace(t)
	defer closeWorkspace(t, workspace)

	result, err := workspace.ResolveVariable(
		context.Background(),
		"premium-message",
		map[string]any{"user": map[string]any{"tier": map[string]any{"bad": "shape"}}},
		&ResolveOptions{SkipContextValidation: true},
	)
	if err != nil {
		t.Fatal(err)
	}
	if result.ValueKey != "control" {
		t.Fatalf("value key = %q", result.ValueKey)
	}
}

func TestLoadRejectsInvalidLintMode(t *testing.T) {
	_, err := Load(context.Background(), basicWorkspace(), &LoadOptions{Lint: LintMode("warn")})
	if err == nil {
		t.Fatal("expected invalid lint mode error")
	}
	if !strings.Contains(err.Error(), "lint must be 'deny' or 'skip'") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestRefreshingWorkspaceRefreshesLocalSource(t *testing.T) {
	root := t.TempDir()
	writeWorkspace(t, root, "hello")

	workspace, err := LoadRefreshing(context.Background(), root, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer closeRefreshingWorkspace(t, workspace)

	initial, err := workspace.ResolveVariable(context.Background(), "message", map[string]any{}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if initial.Value != "hello" {
		t.Fatalf("initial value = %#v", initial.Value)
	}

	writeWorkspace(t, root, "updated")
	outcome, err := workspace.RefreshNow(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if outcome != "refreshed" && outcome != "unchanged" {
		t.Fatalf("unexpected refresh outcome: %s", outcome)
	}

	refreshed, err := workspace.ResolveVariable(context.Background(), "message", map[string]any{}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if refreshed.Value != "updated" {
		t.Fatalf("refreshed value = %#v", refreshed.Value)
	}

	status, err := workspace.Status(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if status.LastSuccess == nil {
		t.Fatal("last success was nil")
	}
	if status.ConsecutiveFailures != 0 {
		t.Fatalf("consecutive failures = %d", status.ConsecutiveFailures)
	}

	if err := workspace.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	_, err = workspace.ResolveVariable(context.Background(), "message", map[string]any{}, nil)
	if err == nil {
		t.Fatal("expected closed refreshing workspace resolution to fail")
	}
}

func TestSharedContractCases(t *testing.T) {
	for _, sdkCase := range contractCases(t) {
		t.Run(sdkCase.Name, func(t *testing.T) {
			actual, err := runContractCase(t, sdkCase)
			if sdkCase.Expect.OK {
				if err != nil {
					t.Fatal(err)
				}
				assertExpectedSubset(t, actual, sdkCase.Expect)
			} else {
				if err == nil {
					t.Fatal("expected contract case to fail")
				}
				if !strings.Contains(err.Error(), sdkCase.Expect.Error.Contains) {
					t.Fatalf("error %q did not contain %q", err.Error(), sdkCase.Expect.Error.Contains)
				}
			}
		})
	}
}

func TestPublicAPIExportsExpectedNames(t *testing.T) {
	version, err := Version()
	if err != nil {
		t.Fatal(err)
	}
	if version == "" || !strings.Contains(version, "-alpha.") {
		t.Fatalf("unexpected version: %q", version)
	}
}

type contractCase struct {
	Name      string         `json:"name"`
	Operation string         `json:"operation"`
	Workspace string         `json:"workspace"`
	ID        string         `json:"id"`
	Context   map[string]any `json:"context"`
	Expect    contractExpect `json:"expect"`
}

type contractExpect struct {
	OK          bool                  `json:"ok"`
	Diagnostics *int                  `json:"diagnostics"`
	Result      map[string]any        `json:"result"`
	Error       contractErrorExpected `json:"error"`
}

type contractErrorExpected struct {
	Contains string `json:"contains"`
}

func runContractCase(t *testing.T, sdkCase contractCase) (map[string]any, error) {
	t.Helper()
	source := filepath.Join(repoRoot(t), filepath.FromSlash(sdkCase.Workspace))

	switch sdkCase.Operation {
	case "load_workspace":
		workspace, err := Load(context.Background(), source, nil)
		if err != nil {
			return nil, err
		}
		defer closeWorkspace(t, workspace)
		return map[string]any{"ok": true}, nil
	case "lint_workspace":
		workspace, err := Inspect(context.Background(), source, nil)
		if err != nil {
			return nil, err
		}
		defer closeWorkspace(t, workspace)
		lint, err := workspace.Lint(context.Background())
		if err != nil {
			return nil, err
		}
		return map[string]any{"diagnostics": len(lint.Diagnostics)}, nil
	case "resolve_variable":
		workspace, err := Load(context.Background(), source, nil)
		if err != nil {
			return nil, err
		}
		defer closeWorkspace(t, workspace)
		result, err := workspace.ResolveVariable(context.Background(), sdkCase.ID, sdkCase.Context, nil)
		if err != nil {
			return nil, err
		}
		return map[string]any{
			"id":        result.ID,
			"value_key": result.ValueKey,
			"value":     result.Value,
		}, nil
	case "resolve_qualifier":
		workspace, err := Load(context.Background(), source, nil)
		if err != nil {
			return nil, err
		}
		defer closeWorkspace(t, workspace)
		result, err := workspace.ResolveQualifier(context.Background(), sdkCase.ID, sdkCase.Context, nil)
		if err != nil {
			return nil, err
		}
		return map[string]any{"id": result.ID, "value": result.Value}, nil
	default:
		t.Fatalf("unsupported contract operation: %s", sdkCase.Operation)
		return nil, nil
	}
}

func contractCases(t *testing.T) []contractCase {
	t.Helper()
	file, err := os.Open(filepath.Join(repoRoot(t), "tests", "sdk-contract", "cases.jsonl"))
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()

	var cases []contractCase
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		var sdkCase contractCase
		if err := json.Unmarshal([]byte(line), &sdkCase); err != nil {
			t.Fatal(err)
		}
		cases = append(cases, sdkCase)
	}
	if err := scanner.Err(); err != nil {
		t.Fatal(err)
	}
	return cases
}

func assertExpectedSubset(t *testing.T, actual map[string]any, expect contractExpect) {
	t.Helper()
	if expect.Diagnostics != nil {
		if actual["diagnostics"] != *expect.Diagnostics {
			t.Fatalf("diagnostics = %#v, want %d", actual["diagnostics"], *expect.Diagnostics)
		}
	}
	if expect.Result != nil {
		assertSubset(t, actual, expect.Result)
	}
}

func assertSubset(t *testing.T, actual any, expected any) {
	t.Helper()
	expectedMap, ok := expected.(map[string]any)
	if !ok {
		if !reflect.DeepEqual(actual, expected) {
			t.Fatalf("actual = %#v, expected = %#v", actual, expected)
		}
		return
	}
	actualMap, ok := actual.(map[string]any)
	if !ok {
		t.Fatalf("actual = %#v, expected object", actual)
	}
	for key, value := range expectedMap {
		actualValue, ok := actualMap[key]
		if !ok {
			t.Fatalf("actual object missing %q", key)
		}
		assertSubset(t, actualValue, value)
	}
}

func loadBasicWorkspace(t *testing.T) *Workspace {
	t.Helper()
	workspace, err := Load(context.Background(), basicWorkspace(), nil)
	if err != nil {
		t.Fatal(err)
	}
	return workspace
}

func closeWorkspace(t *testing.T, workspace *Workspace) {
	t.Helper()
	if err := workspace.Close(); err != nil {
		t.Fatal(err)
	}
}

func closeRefreshingWorkspace(t *testing.T, workspace *RefreshingWorkspace) {
	t.Helper()
	if err := workspace.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
}

func basicWorkspace() string {
	return filepath.Join(repoRootForCaller(), "examples", "basic")
}

func repoRoot(t *testing.T) string {
	t.Helper()
	return repoRootForCaller()
}

func repoRootForCaller() string {
	wd, err := os.Getwd()
	if err != nil {
		panic(err)
	}
	return filepath.Clean(filepath.Join(wd, "..", ".."))
}

func writeWorkspace(t *testing.T, root string, message string) {
	t.Helper()
	variables := filepath.Join(root, "variables")
	if err := os.MkdirAll(variables, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "rototo-workspace.toml"), []byte("schema_version = 1\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	contents := `schema_version = 1

description = "Message"
type = "string"

[values]
default = "` + message + `"

[resolve]
default = "default"
`
	if err := os.WriteFile(filepath.Join(variables, "message.toml"), []byte(contents), 0o644); err != nil {
		t.Fatal(err)
	}
}
