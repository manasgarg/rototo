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

func TestPackageExposesGoRuntimeResolutionAPI(t *testing.T) {
	pkg := loadBasicPackage(t)
	defer closePackage(t, pkg)

	variable, err := pkg.ResolveVariable(
		"premium-message",
		map[string]any{"user": map[string]any{"tier": "premium"}},
		nil,
	)
	if err != nil {
		t.Fatal(err)
	}
	qualifier, err := pkg.ResolveQualifier(
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
	if !reflect.DeepEqual(variable.Source, map[string]any{"kind": "literal"}) {
		t.Fatalf("source = %#v", variable.Source)
	}
	if variable.Value != "Welcome back, premium member." {
		t.Fatalf("value = %#v", variable.Value)
	}
	if !qualifier {
		t.Fatalf("qualifier value = false")
	}
}

func TestInspectedPackageCanLintButNotResolve(t *testing.T) {
	pkg, err := Inspect(context.Background(), basicPackage(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer closePackage(t, pkg)

	lint, err := pkg.Lint(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(lint.Diagnostics) != 0 {
		t.Fatalf("diagnostics = %#v", lint.Diagnostics)
	}

	_, err = pkg.ResolveVariable("premium-message", map[string]any{}, nil)
	if err == nil {
		t.Fatal("expected inspected package resolution to fail")
	}
	if !strings.Contains(err.Error(), "package was loaded without a runtime model") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestContextValidationCanBeSkipped(t *testing.T) {
	pkg := loadBasicPackage(t)
	defer closePackage(t, pkg)

	result, err := pkg.ResolveVariable(
		"premium-message",
		map[string]any{"user": map[string]any{"tier": map[string]any{"bad": "shape"}}},
		&ResolveOptions{SkipContextValidation: true},
	)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(result.Source, map[string]any{"kind": "literal"}) {
		t.Fatalf("source = %#v", result.Source)
	}
}

func TestLoadRejectsInvalidLintMode(t *testing.T) {
	_, err := Load(context.Background(), basicPackage(), &LoadOptions{Lint: LintMode("warn")})
	if err == nil {
		t.Fatal("expected invalid lint mode error")
	}
	if !strings.Contains(err.Error(), "lint must be 'deny' or 'skip'") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestRefreshingPackageRefreshesLocalSource(t *testing.T) {
	root := t.TempDir()
	writePackage(t, root, "hello")

	pkg, err := LoadRefreshing(context.Background(), root, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer closeRefreshingPackage(t, pkg)

	initial, err := pkg.ResolveVariable("message", map[string]any{}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if initial.Value != "hello" {
		t.Fatalf("initial value = %#v", initial.Value)
	}

	writePackage(t, root, "updated")
	outcome, err := pkg.RefreshNow(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if outcome != "refreshed" && outcome != "unchanged" {
		t.Fatalf("unexpected refresh outcome: %s", outcome)
	}

	refreshed, err := pkg.ResolveVariable("message", map[string]any{}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if refreshed.Value != "updated" {
		t.Fatalf("refreshed value = %#v", refreshed.Value)
	}

	status, err := pkg.Status(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if status.LastSuccess == nil {
		t.Fatal("last success was nil")
	}
	if status.ConsecutiveFailures != 0 {
		t.Fatalf("consecutive failures = %d", status.ConsecutiveFailures)
	}

	if err := pkg.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	_, err = pkg.ResolveVariable("message", map[string]any{}, nil)
	if err == nil {
		t.Fatal("expected closed refreshing package resolution to fail")
	}
}

func TestRefreshingPackageIdentitySnapshotAndEvents(t *testing.T) {
	root := t.TempDir()
	writePackage(t, root, "hello")

	pkg, err := LoadRefreshing(context.Background(), root, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer closeRefreshingPackage(t, pkg)

	identity, err := pkg.Identity(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	// A local directory has no fingerprint, so no derived release id.
	if identity.ReleaseID != nil {
		t.Fatalf("expected nil release id, got %v", *identity.ReleaseID)
	}

	snapshot, err := pkg.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if snapshot.LastSuccess == nil {
		t.Fatal("snapshot last success was nil")
	}
	if snapshot.LastEvent == nil || snapshot.LastEvent.EventType != "loaded" {
		t.Fatalf("expected loaded last event, got %#v", snapshot.LastEvent)
	}

	eventsCtx, cancelEvents := context.WithCancel(context.Background())
	defer cancelEvents()
	events, err := pkg.RefreshEvents(eventsCtx)
	if err != nil {
		t.Fatal(err)
	}

	writePackage(t, root, "updated")
	if _, err := pkg.RefreshNow(context.Background()); err != nil {
		t.Fatal(err)
	}

	var refreshed *RefreshEvent
	for event := range events {
		if event.EventType == "refreshed" {
			captured := event
			refreshed = &captured
			break
		}
	}
	if refreshed == nil {
		t.Fatal("did not observe a refreshed event")
	}
	if refreshed.SchemaVersion != 1 {
		t.Fatalf("schema version = %d", refreshed.SchemaVersion)
	}
	if refreshed.Sdk.Language != "rust" {
		t.Fatalf("sdk language = %s", refreshed.Sdk.Language)
	}
	if refreshed.Current == nil {
		t.Fatal("refreshed event had no current identity")
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
	Package   string         `json:"package"`
	ID        string         `json:"id"`
	Context   map[string]any `json:"context"`
	Expect    contractExpect `json:"expect"`
}

type contractExpect struct {
	OK          bool                  `json:"ok"`
	Diagnostics *int                  `json:"diagnostics"`
	Result      any                   `json:"result"`
	Error       contractErrorExpected `json:"error"`
}

type contractErrorExpected struct {
	Contains string `json:"contains"`
}

func runContractCase(t *testing.T, sdkCase contractCase) (any, error) {
	t.Helper()
	source := filepath.Join(repoRoot(t), filepath.FromSlash(sdkCase.Package))

	switch sdkCase.Operation {
	case "load_package":
		pkg, err := Load(context.Background(), source, nil)
		if err != nil {
			return nil, err
		}
		defer closePackage(t, pkg)
		return map[string]any{"ok": true}, nil
	case "lint_package":
		pkg, err := Inspect(context.Background(), source, nil)
		if err != nil {
			return nil, err
		}
		defer closePackage(t, pkg)
		lint, err := pkg.Lint(context.Background())
		if err != nil {
			return nil, err
		}
		return map[string]any{"diagnostics": len(lint.Diagnostics)}, nil
	case "resolve_variable":
		pkg, err := Load(context.Background(), source, nil)
		if err != nil {
			return nil, err
		}
		defer closePackage(t, pkg)
		result, err := pkg.ResolveVariable(sdkCase.ID, sdkCase.Context, nil)
		if err != nil {
			return nil, err
		}
		return map[string]any{
			"id":     result.ID,
			"value":  result.Value,
			"source": result.Source,
		}, nil
	case "resolve_qualifier":
		pkg, err := Load(context.Background(), source, nil)
		if err != nil {
			return nil, err
		}
		defer closePackage(t, pkg)
		result, err := pkg.ResolveQualifier(sdkCase.ID, sdkCase.Context, nil)
		if err != nil {
			return nil, err
		}
		return result, nil
	case "package_identity":
		pkg, err := Load(context.Background(), source, nil)
		if err != nil {
			return nil, err
		}
		defer closePackage(t, pkg)
		identity, err := pkg.Identity()
		if err != nil {
			return nil, err
		}
		var releaseID any
		if identity.ReleaseID != nil {
			releaseID = *identity.ReleaseID
		}
		return map[string]any{
			"releaseId": releaseID,
			"immutable": identity.Immutable,
		}, nil
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

func assertExpectedSubset(t *testing.T, actual any, expect contractExpect) {
	t.Helper()
	if expect.Diagnostics != nil {
		actualMap, ok := actual.(map[string]any)
		if !ok {
			t.Fatalf("actual = %#v, expected object", actual)
		}
		if actualMap["diagnostics"] != *expect.Diagnostics {
			t.Fatalf("diagnostics = %#v, want %d", actualMap["diagnostics"], *expect.Diagnostics)
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

func loadBasicPackage(t *testing.T) *Package {
	t.Helper()
	pkg, err := Load(context.Background(), basicPackage(), nil)
	if err != nil {
		t.Fatal(err)
	}
	return pkg
}

func closePackage(t *testing.T, pkg *Package) {
	t.Helper()
	if err := pkg.Close(); err != nil {
		t.Fatal(err)
	}
}

func closeRefreshingPackage(t *testing.T, pkg *RefreshingPackage) {
	t.Helper()
	if err := pkg.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
}

func basicPackage() string {
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

func writePackage(t *testing.T, root string, message string) {
	t.Helper()
	variables := filepath.Join(root, "variables")
	if err := os.MkdirAll(variables, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "rototo-package.toml"), []byte("schema_version = 1\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	contents := `schema_version = 1

description = "Message"
type = "string"

[resolve]
default = "` + message + `"
`
	if err := os.WriteFile(filepath.Join(variables, "message.toml"), []byte(contents), 0o644); err != nil {
		t.Fatal(err)
	}
}
