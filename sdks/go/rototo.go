package rototo

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"sync"
)

type nativeHandle uintptr

// Error is returned when rototo rejects a package, source, context, or
// resolution request.
type Error struct {
	Message string
}

func (e *Error) Error() string {
	return e.Message
}

// LintMode controls how package lint is handled during load.
type LintMode string

const (
	LintDeny LintMode = "deny"
	LintSkip LintMode = "skip"
)

// LoadOptions configures Package loading.
type LoadOptions struct {
	PackageToken string
	Lint         LintMode
}

// InspectOptions configures Package inspection.
type InspectOptions struct {
	PackageToken string
}

// ResolveOptions configures a single resolution call.
type ResolveOptions struct {
	SkipContextValidation bool
}

// RefreshingPackageOptions configures RefreshingPackage loading.
type RefreshingPackageOptions struct {
	PeriodSeconds *float64
	PackageToken  string
	Lint          LintMode
}

// Package is a loaded rototo package handle.
type Package struct {
	mu     sync.RWMutex
	handle nativeHandle
}

// VariableResolution is the selected variable value.
type VariableResolution struct {
	ID     string `json:"id"`
	Value  any    `json:"value"`
	Source any    `json:"source"`
}

// PackageLint is the lint result for a loaded or inspected package.
type PackageLint struct {
	Root        string `json:"root"`
	Diagnostics []any  `json:"diagnostics"`
}

// RefreshStatus is the current refresh state.
type RefreshStatus struct {
	CurrentFingerprint  any      `json:"currentFingerprint"`
	LastSuccess         *float64 `json:"lastSuccess"`
	LastAttempt         *float64 `json:"lastAttempt"`
	ConsecutiveFailures uint64   `json:"consecutiveFailures"`
	LastError           *string  `json:"lastError"`
	Refreshing          bool     `json:"refreshing"`
	Immutable           bool     `json:"immutable"`
}

// RefreshingPackage is a package handle with background refresh support.
type RefreshingPackage struct {
	mu     sync.RWMutex
	handle nativeHandle
}

// Version returns the canonical rototo version exposed by the native SDK.
func Version() (string, error) {
	return nativeVersion()
}

// Load stages, lints, and loads a runtime package.
func Load(ctx context.Context, source string, options *LoadOptions) (*Package, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	if options == nil {
		options = &LoadOptions{}
	}
	lint := options.Lint
	if lint == "" {
		lint = LintDeny
	}
	handle, err := nativePackageLoad(source, options.PackageToken, string(lint))
	if err != nil {
		return nil, err
	}
	if err := checkContext(ctx); err != nil {
		nativePackageFree(handle)
		return nil, err
	}
	return &Package{handle: handle}, nil
}

// Inspect stages and inspects a package without compiling the runtime model.
func Inspect(ctx context.Context, source string, options *InspectOptions) (*Package, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	if options == nil {
		options = &InspectOptions{}
	}
	handle, err := nativePackageInspect(source, options.PackageToken)
	if err != nil {
		return nil, err
	}
	if err := checkContext(ctx); err != nil {
		nativePackageFree(handle)
		return nil, err
	}
	return &Package{handle: handle}, nil
}

// Root returns the staged package root path.
func (w *Package) Root() (string, error) {
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return "", err
	}
	defer unlock()
	return nativePackageRoot(handle)
}

// Lint runs package lint for this handle.
func (w *Package) Lint(ctx context.Context) (*PackageLint, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return nil, err
	}
	defer unlock()
	text, err := nativePackageLint(handle)
	if err != nil {
		return nil, err
	}
	var lint PackageLint
	if err := json.Unmarshal([]byte(text), &lint); err != nil {
		return nil, err
	}
	return &lint, checkContext(ctx)
}

// ResolveVariable resolves a variable with a JSON-object context.
func (w *Package) ResolveVariable(
	id string,
	evaluationContext map[string]any,
	options *ResolveOptions,
) (*VariableResolution, error) {
	contextJSON, err := marshalContext(evaluationContext)
	if err != nil {
		return nil, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return nil, err
	}
	defer unlock()
	text, err := nativePackageResolveVariable(handle, id, contextJSON, validateContext(options))
	if err != nil {
		return nil, err
	}
	var resolution VariableResolution
	if err := json.Unmarshal([]byte(text), &resolution); err != nil {
		return nil, err
	}
	return &resolution, nil
}

// ResolveQualifier resolves a qualifier with a JSON-object context.
func (w *Package) ResolveQualifier(
	id string,
	evaluationContext map[string]any,
	options *ResolveOptions,
) (bool, error) {
	contextJSON, err := marshalContext(evaluationContext)
	if err != nil {
		return false, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return false, err
	}
	defer unlock()
	text, err := nativePackageResolveQualifier(handle, id, contextJSON, validateContext(options))
	if err != nil {
		return false, err
	}
	var value bool
	if err := json.Unmarshal([]byte(text), &value); err != nil {
		return false, err
	}
	return value, nil
}

// Close releases the native package handle.
func (w *Package) Close() error {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.handle == 0 {
		return nil
	}
	nativePackageFree(w.handle)
	w.handle = 0
	return nil
}

// LoadRefreshing stages, lints, and loads a refreshing package.
func LoadRefreshing(
	ctx context.Context,
	source string,
	options *RefreshingPackageOptions,
) (*RefreshingPackage, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	if options == nil {
		options = &RefreshingPackageOptions{}
	}
	lint := options.Lint
	if lint == "" {
		lint = LintDeny
	}
	handle, err := nativeRefreshingPackageLoad(
		source,
		options.PeriodSeconds,
		options.PackageToken,
		string(lint),
	)
	if err != nil {
		return nil, err
	}
	if err := checkContext(ctx); err != nil {
		nativeRefreshingPackageFree(handle)
		return nil, err
	}
	return &RefreshingPackage{handle: handle}, nil
}

// ResolveVariable resolves a variable against the current active package.
func (w *RefreshingPackage) ResolveVariable(
	id string,
	evaluationContext map[string]any,
	options *ResolveOptions,
) (*VariableResolution, error) {
	contextJSON, err := marshalContext(evaluationContext)
	if err != nil {
		return nil, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return nil, err
	}
	defer unlock()
	text, err := nativeRefreshingPackageResolveVariable(
		handle,
		id,
		contextJSON,
		validateContext(options),
	)
	if err != nil {
		return nil, err
	}
	var resolution VariableResolution
	if err := json.Unmarshal([]byte(text), &resolution); err != nil {
		return nil, err
	}
	return &resolution, nil
}

// ResolveQualifier resolves a qualifier against the current active package.
func (w *RefreshingPackage) ResolveQualifier(
	id string,
	evaluationContext map[string]any,
	options *ResolveOptions,
) (bool, error) {
	contextJSON, err := marshalContext(evaluationContext)
	if err != nil {
		return false, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return false, err
	}
	defer unlock()
	text, err := nativeRefreshingPackageResolveQualifier(
		handle,
		id,
		contextJSON,
		validateContext(options),
	)
	if err != nil {
		return false, err
	}
	var value bool
	if err := json.Unmarshal([]byte(text), &value); err != nil {
		return false, err
	}
	return value, nil
}

// RefreshNow refreshes the package immediately and returns "unchanged",
// "refreshed", or "immutable".
func (w *RefreshingPackage) RefreshNow(ctx context.Context) (string, error) {
	if err := checkContext(ctx); err != nil {
		return "", err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return "", err
	}
	defer unlock()
	outcome, err := nativeRefreshingPackageRefreshNow(handle)
	if err != nil {
		return "", err
	}
	return outcome, checkContext(ctx)
}

// Status returns the current refresh state.
func (w *RefreshingPackage) Status(ctx context.Context) (*RefreshStatus, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return nil, err
	}
	defer unlock()
	text, err := nativeRefreshingPackageStatus(handle)
	if err != nil {
		return nil, err
	}
	var status RefreshStatus
	if err := json.Unmarshal([]byte(text), &status); err != nil {
		return nil, err
	}
	return &status, checkContext(ctx)
}

// Shutdown stops background refresh without freeing the handle.
func (w *RefreshingPackage) Shutdown(ctx context.Context) error {
	if err := checkContext(ctx); err != nil {
		return err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return err
	}
	defer unlock()
	if err := nativeRefreshingPackageShutdown(handle); err != nil {
		return err
	}
	return checkContext(ctx)
}

// Close shuts down and releases the native refreshing package handle.
func (w *RefreshingPackage) Close(ctx context.Context) error {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.handle == 0 {
		return nil
	}
	var shutdownErr error
	if err := checkContext(ctx); err == nil {
		shutdownErr = nativeRefreshingPackageShutdown(w.handle)
	} else {
		shutdownErr = err
	}
	nativeRefreshingPackageFree(w.handle)
	w.handle = 0
	return shutdownErr
}

func (w *Package) activeHandle() (nativeHandle, func(), error) {
	if w == nil {
		return 0, nil, errors.New("package is nil")
	}
	w.mu.RLock()
	if w.handle == 0 {
		w.mu.RUnlock()
		return 0, nil, &Error{Message: "package has been closed"}
	}
	return w.handle, w.mu.RUnlock, nil
}

func (w *RefreshingPackage) activeHandle() (nativeHandle, func(), error) {
	if w == nil {
		return 0, nil, errors.New("refreshing package is nil")
	}
	w.mu.RLock()
	if w.handle == 0 {
		w.mu.RUnlock()
		return 0, nil, &Error{Message: "refreshing package has been closed"}
	}
	return w.handle, w.mu.RUnlock, nil
}

func checkContext(ctx context.Context) error {
	if ctx == nil {
		return nil
	}
	return ctx.Err()
}

func marshalContext(context map[string]any) (string, error) {
	if context == nil {
		context = map[string]any{}
	}
	data, err := json.Marshal(context)
	if err != nil {
		return "", err
	}
	if len(data) == 0 || data[0] != '{' {
		return "", fmt.Errorf("evaluation context must be a JSON object")
	}
	return string(data), nil
}

func validateContext(options *ResolveOptions) bool {
	return options == nil || !options.SkipContextValidation
}

func nativeError(message string) error {
	if message == "" {
		return nil
	}
	return &Error{Message: message}
}
