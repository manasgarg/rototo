//go:build !cgo

package rototo

import "errors"

func nativeVersion() (string, error) {
	return "", cgoDisabled()
}

func nativePackageLoad(source, packageToken, lint string) (nativeHandle, error) {
	return 0, cgoDisabled()
}

func nativePackageInspect(source, packageToken string) (nativeHandle, error) {
	return 0, cgoDisabled()
}

func nativePackageRoot(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativePackageIdentity(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativePackageLint(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativePackageResolveVariable(handle nativeHandle, id, contextJSON string, validateContext bool, trace bool, tenant string) (string, error) {
	return "", cgoDisabled()
}

func nativePackageFree(handle nativeHandle) {}

func nativeRefreshingPackageLoad(source string, periodSeconds *float64, packageToken, lint string) (nativeHandle, error) {
	return 0, cgoDisabled()
}

func nativeRefreshingPackageResolveVariable(handle nativeHandle, id, contextJSON string, validateContext bool, trace bool, tenant string) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingPackageRefreshNow(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingPackageStatus(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingPackageIdentity(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingPackageSnapshot(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingPackageSubscribeEvents(handle nativeHandle) (nativeHandle, error) {
	return 0, cgoDisabled()
}

func nativeRefreshEventsNext(handle nativeHandle) (string, bool, error) {
	return "", false, cgoDisabled()
}

func nativeRefreshEventsFree(handle nativeHandle) {}

func nativeRefreshingPackageSubscribeTraceEvents(handle nativeHandle) (nativeHandle, error) {
	return 0, cgoDisabled()
}

func nativeTraceEventsNext(handle nativeHandle) (string, bool, error) {
	return "", false, cgoDisabled()
}

func nativeTraceEventsFree(handle nativeHandle) {}

func nativeRefreshingPackageShutdown(handle nativeHandle) error {
	return cgoDisabled()
}

func nativeRefreshingPackageFree(handle nativeHandle) {}

func cgoDisabled() error {
	return errors.New("rototo Go SDK requires cgo")
}
