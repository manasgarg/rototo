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

func nativePackageLint(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativePackageResolveVariable(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	return "", cgoDisabled()
}

func nativePackageResolveQualifier(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	return "", cgoDisabled()
}

func nativePackageFree(handle nativeHandle) {}

func nativeRefreshingPackageLoad(source string, periodSeconds *float64, packageToken, lint string) (nativeHandle, error) {
	return 0, cgoDisabled()
}

func nativeRefreshingPackageResolveVariable(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingPackageResolveQualifier(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingPackageRefreshNow(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingPackageStatus(handle nativeHandle) (string, error) {
	return "", cgoDisabled()
}

func nativeRefreshingPackageShutdown(handle nativeHandle) error {
	return cgoDisabled()
}

func nativeRefreshingPackageFree(handle nativeHandle) {}

func cgoDisabled() error {
	return errors.New("rototo Go SDK requires cgo")
}
