package rototo

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"sync"
)

type nativeHandle uintptr

// Error is returned when rototo rejects a workspace, source, context, or
// resolution request.
type Error struct {
	Message string
}

func (e *Error) Error() string {
	return e.Message
}

// LintMode controls how workspace lint is handled during load.
type LintMode string

const (
	LintDeny LintMode = "deny"
	LintSkip LintMode = "skip"
)

// LoadOptions configures Workspace loading.
type LoadOptions struct {
	WorkspaceToken string
	Lint           LintMode
}

// InspectOptions configures Workspace inspection.
type InspectOptions struct {
	WorkspaceToken string
}

// ResolveOptions configures a single resolution call.
type ResolveOptions struct {
	SkipContextValidation bool
}

// RefreshingWorkspaceOptions configures RefreshingWorkspace loading.
type RefreshingWorkspaceOptions struct {
	PeriodSeconds  *float64
	WorkspaceToken string
	Lint           LintMode
}

// Workspace is a loaded rototo workspace handle.
type Workspace struct {
	mu     sync.RWMutex
	handle nativeHandle
}

// VariableResolution is the selected variable value.
type VariableResolution struct {
	ID       string `json:"id"`
	ValueKey string `json:"valueKey"`
	Value    any    `json:"value"`
}

// QualifierResolution is the evaluated qualifier result.
type QualifierResolution struct {
	ID    string `json:"id"`
	Value bool   `json:"value"`
}

// WorkspaceLint is the lint result for a loaded or inspected workspace.
type WorkspaceLint struct {
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

// RefreshingWorkspace is a workspace handle with background refresh support.
type RefreshingWorkspace struct {
	mu     sync.RWMutex
	handle nativeHandle
}

// Version returns the canonical rototo version exposed by the native SDK.
func Version() (string, error) {
	return nativeVersion()
}

// Load stages, lints, and loads a runtime workspace.
func Load(ctx context.Context, source string, options *LoadOptions) (*Workspace, error) {
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
	handle, err := nativeWorkspaceLoad(source, options.WorkspaceToken, string(lint))
	if err != nil {
		return nil, err
	}
	if err := checkContext(ctx); err != nil {
		nativeWorkspaceFree(handle)
		return nil, err
	}
	return &Workspace{handle: handle}, nil
}

// Inspect stages and inspects a workspace without compiling the runtime model.
func Inspect(ctx context.Context, source string, options *InspectOptions) (*Workspace, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	if options == nil {
		options = &InspectOptions{}
	}
	handle, err := nativeWorkspaceInspect(source, options.WorkspaceToken)
	if err != nil {
		return nil, err
	}
	if err := checkContext(ctx); err != nil {
		nativeWorkspaceFree(handle)
		return nil, err
	}
	return &Workspace{handle: handle}, nil
}

// Root returns the staged workspace root path.
func (w *Workspace) Root() (string, error) {
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return "", err
	}
	defer unlock()
	return nativeWorkspaceRoot(handle)
}

// Lint runs workspace lint for this handle.
func (w *Workspace) Lint(ctx context.Context) (*WorkspaceLint, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return nil, err
	}
	defer unlock()
	text, err := nativeWorkspaceLint(handle)
	if err != nil {
		return nil, err
	}
	var lint WorkspaceLint
	if err := json.Unmarshal([]byte(text), &lint); err != nil {
		return nil, err
	}
	return &lint, checkContext(ctx)
}

// ResolveVariable resolves a variable with a JSON-object context.
func (w *Workspace) ResolveVariable(
	ctx context.Context,
	id string,
	resolveContext map[string]any,
	options *ResolveOptions,
) (*VariableResolution, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	contextJSON, err := marshalContext(resolveContext)
	if err != nil {
		return nil, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return nil, err
	}
	defer unlock()
	text, err := nativeWorkspaceResolveVariable(handle, id, contextJSON, validateContext(options))
	if err != nil {
		return nil, err
	}
	var resolution VariableResolution
	if err := json.Unmarshal([]byte(text), &resolution); err != nil {
		return nil, err
	}
	return &resolution, checkContext(ctx)
}

// ResolveQualifier resolves a qualifier with a JSON-object context.
func (w *Workspace) ResolveQualifier(
	ctx context.Context,
	id string,
	resolveContext map[string]any,
	options *ResolveOptions,
) (*QualifierResolution, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	contextJSON, err := marshalContext(resolveContext)
	if err != nil {
		return nil, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return nil, err
	}
	defer unlock()
	text, err := nativeWorkspaceResolveQualifier(handle, id, contextJSON, validateContext(options))
	if err != nil {
		return nil, err
	}
	var resolution QualifierResolution
	if err := json.Unmarshal([]byte(text), &resolution); err != nil {
		return nil, err
	}
	return &resolution, checkContext(ctx)
}

// Close releases the native workspace handle.
func (w *Workspace) Close() error {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.handle == 0 {
		return nil
	}
	nativeWorkspaceFree(w.handle)
	w.handle = 0
	return nil
}

// LoadRefreshing stages, lints, and loads a refreshing workspace.
func LoadRefreshing(
	ctx context.Context,
	source string,
	options *RefreshingWorkspaceOptions,
) (*RefreshingWorkspace, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	if options == nil {
		options = &RefreshingWorkspaceOptions{}
	}
	lint := options.Lint
	if lint == "" {
		lint = LintDeny
	}
	handle, err := nativeRefreshingWorkspaceLoad(
		source,
		options.PeriodSeconds,
		options.WorkspaceToken,
		string(lint),
	)
	if err != nil {
		return nil, err
	}
	if err := checkContext(ctx); err != nil {
		nativeRefreshingWorkspaceFree(handle)
		return nil, err
	}
	return &RefreshingWorkspace{handle: handle}, nil
}

// ResolveVariable resolves a variable against the current active workspace.
func (w *RefreshingWorkspace) ResolveVariable(
	ctx context.Context,
	id string,
	resolveContext map[string]any,
	options *ResolveOptions,
) (*VariableResolution, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	contextJSON, err := marshalContext(resolveContext)
	if err != nil {
		return nil, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return nil, err
	}
	defer unlock()
	text, err := nativeRefreshingWorkspaceResolveVariable(
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
	return &resolution, checkContext(ctx)
}

// ResolveQualifier resolves a qualifier against the current active workspace.
func (w *RefreshingWorkspace) ResolveQualifier(
	ctx context.Context,
	id string,
	resolveContext map[string]any,
	options *ResolveOptions,
) (*QualifierResolution, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	contextJSON, err := marshalContext(resolveContext)
	if err != nil {
		return nil, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return nil, err
	}
	defer unlock()
	text, err := nativeRefreshingWorkspaceResolveQualifier(
		handle,
		id,
		contextJSON,
		validateContext(options),
	)
	if err != nil {
		return nil, err
	}
	var resolution QualifierResolution
	if err := json.Unmarshal([]byte(text), &resolution); err != nil {
		return nil, err
	}
	return &resolution, checkContext(ctx)
}

// RefreshNow refreshes the workspace immediately and returns "unchanged",
// "refreshed", or "immutable".
func (w *RefreshingWorkspace) RefreshNow(ctx context.Context) (string, error) {
	if err := checkContext(ctx); err != nil {
		return "", err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return "", err
	}
	defer unlock()
	outcome, err := nativeRefreshingWorkspaceRefreshNow(handle)
	if err != nil {
		return "", err
	}
	return outcome, checkContext(ctx)
}

// Status returns the current refresh state.
func (w *RefreshingWorkspace) Status(ctx context.Context) (*RefreshStatus, error) {
	if err := checkContext(ctx); err != nil {
		return nil, err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return nil, err
	}
	defer unlock()
	text, err := nativeRefreshingWorkspaceStatus(handle)
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
func (w *RefreshingWorkspace) Shutdown(ctx context.Context) error {
	if err := checkContext(ctx); err != nil {
		return err
	}
	handle, unlock, err := w.activeHandle()
	if err != nil {
		return err
	}
	defer unlock()
	if err := nativeRefreshingWorkspaceShutdown(handle); err != nil {
		return err
	}
	return checkContext(ctx)
}

// Close shuts down and releases the native refreshing workspace handle.
func (w *RefreshingWorkspace) Close(ctx context.Context) error {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.handle == 0 {
		return nil
	}
	var shutdownErr error
	if err := checkContext(ctx); err == nil {
		shutdownErr = nativeRefreshingWorkspaceShutdown(w.handle)
	} else {
		shutdownErr = err
	}
	nativeRefreshingWorkspaceFree(w.handle)
	w.handle = 0
	return shutdownErr
}

func (w *Workspace) activeHandle() (nativeHandle, func(), error) {
	if w == nil {
		return 0, nil, errors.New("workspace is nil")
	}
	w.mu.RLock()
	if w.handle == 0 {
		w.mu.RUnlock()
		return 0, nil, &Error{Message: "workspace has been closed"}
	}
	return w.handle, w.mu.RUnlock, nil
}

func (w *RefreshingWorkspace) activeHandle() (nativeHandle, func(), error) {
	if w == nil {
		return 0, nil, errors.New("refreshing workspace is nil")
	}
	w.mu.RLock()
	if w.handle == 0 {
		w.mu.RUnlock()
		return 0, nil, &Error{Message: "refreshing workspace has been closed"}
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
		return "", fmt.Errorf("resolve context must be a JSON object")
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
