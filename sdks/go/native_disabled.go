//go:build !cgo

package rototo

import "errors"

func nativeVersion() (string, error) {
	return "", cgoDisabled()
}

func nativeWorkspaceLoad(source, workspaceToken, lint string) (nativeHandle, error) {
	return 0, cgoDisabled()
}

func nativeWorkspaceInspect(source, workspaceToken string) (nativeHandle, error) {
	return 0, cgoDisabled()
}

func nativeWorkspaceRoot(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativeWorkspaceLint(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativeWorkspaceResolveVariable(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	return "", cgoDisabled()
}

func nativeWorkspaceResolveQualifier(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	return "", cgoDisabled()
}

func nativeWorkspaceFree(handle nativeHandle) {}

func nativeRefreshingWorkspaceLoad(source string, periodSeconds *float64, workspaceToken, lint string) (nativeHandle, error) {
	return 0, cgoDisabled()
}

func nativeRefreshingWorkspaceResolveVariable(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingWorkspaceResolveQualifier(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingWorkspaceRefreshNow(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingWorkspaceStatus(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingWorkspaceShutdown(handle nativeHandle) error {
	return cgoDisabled()
}

func nativeRefreshingWorkspaceFree(handle nativeHandle) {}

func cgoDisabled() error {
	return errors.New("rototo Go SDK requires cgo")
}
