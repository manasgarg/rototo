//go:build cgo

package rototo

/*
#cgo linux LDFLAGS: -ldl
#include <stdlib.h>
#include <string.h>
#if defined(_WIN32)
#include <windows.h>
static void* rototo_go_open_library(const char* path, char** err) {
    HMODULE handle = LoadLibraryA(path);
    if (handle == NULL) {
        *err = _strdup("failed to load rototo Go native library");
    }
    return (void*)handle;
}
static void* rototo_go_symbol(void* handle, const char* name, char** err) {
    void* symbol = (void*)GetProcAddress((HMODULE)handle, name);
    if (symbol == NULL) {
        *err = _strdup("failed to load symbol from rototo Go native library");
    }
    return symbol;
}
#else
#include <dlfcn.h>
static void* rototo_go_open_library(const char* path, char** err) {
    void* handle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (handle == NULL) {
        const char* message = dlerror();
        if (message != NULL) {
            *err = strdup(message);
        } else {
            *err = strdup("failed to load rototo Go native library");
        }
    }
    return handle;
}
static void* rototo_go_symbol(void* handle, const char* name, char** err) {
    dlerror();
    void* symbol = dlsym(handle, name);
    const char* message = dlerror();
    if (message != NULL) {
        *err = strdup(message);
    }
    return symbol;
}
#endif

typedef struct {
    char* value;
    char* error;
} RototoGoStringResult;

typedef struct {
    void* handle;
    char* error;
} RototoGoHandleResult;

typedef struct {
    char* error;
} RototoGoVoidResult;

typedef RototoGoStringResult (*rototo_go_version_fn)(void);
typedef RototoGoHandleResult (*rototo_go_package_load_fn)(const char*, const char*, const char*);
typedef RototoGoHandleResult (*rototo_go_package_inspect_fn)(const char*, const char*);
typedef RototoGoStringResult (*rototo_go_package_string_fn)(void*);
typedef RototoGoStringResult (*rototo_go_package_resolve_fn)(void*, const char*, const char*, int);
typedef void (*rototo_go_handle_free_fn)(void*);
typedef RototoGoHandleResult (*rototo_go_refreshing_package_load_fn)(const char*, double, int, const char*, const char*);
typedef RototoGoStringResult (*rototo_go_refreshing_package_string_fn)(void*);
typedef RototoGoStringResult (*rototo_go_refreshing_package_resolve_fn)(void*, const char*, const char*, int);
typedef RototoGoVoidResult (*rototo_go_refreshing_package_void_fn)(void*);
typedef void (*rototo_go_string_result_free_fn)(RototoGoStringResult*);
typedef void (*rototo_go_handle_result_free_fn)(RototoGoHandleResult*);
typedef void (*rototo_go_void_result_free_fn)(RototoGoVoidResult*);

static RototoGoStringResult rototo_go_call_version(void* fn) {
    return ((rototo_go_version_fn)fn)();
}
static RototoGoHandleResult rototo_go_call_package_load(void* fn, const char* source, const char* token, const char* lint) {
    return ((rototo_go_package_load_fn)fn)(source, token, lint);
}
static RototoGoHandleResult rototo_go_call_package_inspect(void* fn, const char* source, const char* token) {
    return ((rototo_go_package_inspect_fn)fn)(source, token);
}
static RototoGoStringResult rototo_go_call_package_string(void* fn, void* handle) {
    return ((rototo_go_package_string_fn)fn)(handle);
}
static RototoGoStringResult rototo_go_call_package_resolve(void* fn, void* handle, const char* id, const char* context, int validate_context) {
    return ((rototo_go_package_resolve_fn)fn)(handle, id, context, validate_context);
}
static void rototo_go_call_handle_free(void* fn, void* handle) {
    ((rototo_go_handle_free_fn)fn)(handle);
}
static RototoGoHandleResult rototo_go_call_refreshing_package_load(void* fn, const char* source, double period_seconds, int has_period_seconds, const char* token, const char* lint) {
    return ((rototo_go_refreshing_package_load_fn)fn)(source, period_seconds, has_period_seconds, token, lint);
}
static RototoGoStringResult rototo_go_call_refreshing_package_string(void* fn, void* handle) {
    return ((rototo_go_refreshing_package_string_fn)fn)(handle);
}
static RototoGoStringResult rototo_go_call_refreshing_package_resolve(void* fn, void* handle, const char* id, const char* context, int validate_context) {
    return ((rototo_go_refreshing_package_resolve_fn)fn)(handle, id, context, validate_context);
}
static RototoGoVoidResult rototo_go_call_refreshing_package_void(void* fn, void* handle) {
    return ((rototo_go_refreshing_package_void_fn)fn)(handle);
}
typedef RototoGoHandleResult (*rototo_go_subscribe_fn)(void*);
static RototoGoHandleResult rototo_go_call_subscribe(void* fn, void* handle) {
    return ((rototo_go_subscribe_fn)fn)(handle);
}
static void rototo_go_call_string_result_free(void* fn, RototoGoStringResult* result) {
    ((rototo_go_string_result_free_fn)fn)(result);
}
static void rototo_go_call_handle_result_free(void* fn, RototoGoHandleResult* result) {
    ((rototo_go_handle_result_free_fn)fn)(result);
}
static void rototo_go_call_void_result_free(void* fn, RototoGoVoidResult* result) {
    ((rototo_go_void_result_free_fn)fn)(result);
}
*/
import "C"

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"unsafe"
)

type nativeSymbols struct {
	version                           unsafe.Pointer
	packageLoad                       unsafe.Pointer
	packageInspect                    unsafe.Pointer
	packageRoot                       unsafe.Pointer
	packageIdentity                   unsafe.Pointer
	packageLint                       unsafe.Pointer
	packageResolveVariable            unsafe.Pointer
	packageResolveQualifier           unsafe.Pointer
	packageFree                       unsafe.Pointer
	refreshingPackageLoad             unsafe.Pointer
	refreshingPackageResolveVariable  unsafe.Pointer
	refreshingPackageResolveQualifier unsafe.Pointer
	refreshingPackageRefreshNow       unsafe.Pointer
	refreshingPackageStatus           unsafe.Pointer
	refreshingPackageIdentity         unsafe.Pointer
	refreshingPackageSnapshot         unsafe.Pointer
	refreshingPackageSubscribeEvents  unsafe.Pointer
	refreshEventsNext                 unsafe.Pointer
	refreshEventsFree                 unsafe.Pointer
	refreshingPackageSubscribeTrace   unsafe.Pointer
	traceEventsNext                   unsafe.Pointer
	traceEventsFree                   unsafe.Pointer
	refreshingPackageShutdown         unsafe.Pointer
	refreshingPackageFree             unsafe.Pointer
	stringResultFree                  unsafe.Pointer
	handleResultFree                  unsafe.Pointer
	voidResultFree                    unsafe.Pointer
}

var (
	nativeOnce    sync.Once
	nativeLoadErr error
	native        nativeSymbols
)

func ensureNative() error {
	nativeOnce.Do(func() {
		nativeLoadErr = loadNative()
	})
	return nativeLoadErr
}

func loadNative() error {
	path := nativeLibraryPath()
	cPath := C.CString(path)
	defer C.free(unsafe.Pointer(cPath))

	var cErr *C.char
	handle := C.rototo_go_open_library(cPath, &cErr)
	if cErr != nil {
		defer C.free(unsafe.Pointer(cErr))
		return fmt.Errorf("load rototo Go native library %q: %s", path, C.GoString(cErr))
	}
	if handle == nil {
		return fmt.Errorf("load rototo Go native library %q: returned nil handle", path)
	}

	symbol := func(name string) unsafe.Pointer {
		if nativeLoadErr != nil {
			return nil
		}
		cName := C.CString(name)
		defer C.free(unsafe.Pointer(cName))
		var cErr *C.char
		ptr := C.rototo_go_symbol(handle, cName, &cErr)
		if cErr != nil {
			defer C.free(unsafe.Pointer(cErr))
			nativeLoadErr = fmt.Errorf("load rototo Go native symbol %q: %s", name, C.GoString(cErr))
			return nil
		}
		return ptr
	}

	native.version = symbol("rototo_go_version")
	native.packageLoad = symbol("rototo_go_package_load")
	native.packageInspect = symbol("rototo_go_package_inspect")
	native.packageRoot = symbol("rototo_go_package_root")
	native.packageIdentity = symbol("rototo_go_package_identity")
	native.packageLint = symbol("rototo_go_package_lint")
	native.packageResolveVariable = symbol("rototo_go_package_resolve_variable")
	native.packageResolveQualifier = symbol("rototo_go_package_resolve_qualifier")
	native.packageFree = symbol("rototo_go_package_free")
	native.refreshingPackageLoad = symbol("rototo_go_refreshing_package_load")
	native.refreshingPackageResolveVariable = symbol("rototo_go_refreshing_package_resolve_variable")
	native.refreshingPackageResolveQualifier = symbol("rototo_go_refreshing_package_resolve_qualifier")
	native.refreshingPackageRefreshNow = symbol("rototo_go_refreshing_package_refresh_now")
	native.refreshingPackageStatus = symbol("rototo_go_refreshing_package_status")
	native.refreshingPackageIdentity = symbol("rototo_go_refreshing_package_identity")
	native.refreshingPackageSnapshot = symbol("rototo_go_refreshing_package_snapshot")
	native.refreshingPackageSubscribeEvents = symbol("rototo_go_refreshing_package_subscribe_events")
	native.refreshEventsNext = symbol("rototo_go_refresh_events_next")
	native.refreshEventsFree = symbol("rototo_go_refresh_events_free")
	native.refreshingPackageSubscribeTrace = symbol("rototo_go_refreshing_package_subscribe_trace_events")
	native.traceEventsNext = symbol("rototo_go_trace_events_next")
	native.traceEventsFree = symbol("rototo_go_trace_events_free")
	native.refreshingPackageShutdown = symbol("rototo_go_refreshing_package_shutdown")
	native.refreshingPackageFree = symbol("rototo_go_refreshing_package_free")
	native.stringResultFree = symbol("rototo_go_string_result_free")
	native.handleResultFree = symbol("rototo_go_handle_result_free")
	native.voidResultFree = symbol("rototo_go_void_result_free")
	return nativeLoadErr
}

func nativeLibraryPath() string {
	if path := os.Getenv("ROTOTO_GO_NATIVE_PATH"); path != "" {
		return path
	}
	_, file, _, ok := runtime.Caller(0)
	if ok {
		return filepath.Join(filepath.Dir(file), "native", runtime.GOOS+"-"+runtime.GOARCH, nativeLibraryName())
	}
	return nativeLibraryName()
}

func nativeLibraryName() string {
	switch runtime.GOOS {
	case "darwin":
		return "librototo_go.dylib"
	case "windows":
		return "rototo_go.dll"
	default:
		return "librototo_go.so"
	}
}

func nativeVersion() (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_version(native.version)
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativePackageLoad(source, packageToken, lint string) (nativeHandle, error) {
	if err := ensureNative(); err != nil {
		return 0, err
	}
	cSource := C.CString(source)
	cLint := C.CString(lint)
	defer C.free(unsafe.Pointer(cSource))
	defer C.free(unsafe.Pointer(cLint))
	cToken, freeToken := optionalCString(packageToken)
	defer freeToken()
	result := C.rototo_go_call_package_load(native.packageLoad, cSource, cToken, cLint)
	defer C.rototo_go_call_handle_result_free(native.handleResultFree, &result)
	return handleResult(result)
}

func nativePackageInspect(source, packageToken string) (nativeHandle, error) {
	if err := ensureNative(); err != nil {
		return 0, err
	}
	cSource := C.CString(source)
	defer C.free(unsafe.Pointer(cSource))
	cToken, freeToken := optionalCString(packageToken)
	defer freeToken()
	result := C.rototo_go_call_package_inspect(native.packageInspect, cSource, cToken)
	defer C.rototo_go_call_handle_result_free(native.handleResultFree, &result)
	return handleResult(result)
}

func nativePackageRoot(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_package_string(native.packageRoot, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativePackageIdentity(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_package_string(native.packageIdentity, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativePackageLint(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_package_string(native.packageLint, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativePackageResolveVariable(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	return nativePackageResolve(native.packageResolveVariable, handle, id, contextJSON, validateContext)
}

func nativePackageResolveQualifier(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	return nativePackageResolve(native.packageResolveQualifier, handle, id, contextJSON, validateContext)
}

func nativePackageResolve(fn unsafe.Pointer, handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	cID := C.CString(id)
	cContext := C.CString(contextJSON)
	defer C.free(unsafe.Pointer(cID))
	defer C.free(unsafe.Pointer(cContext))
	result := C.rototo_go_call_package_resolve(fn, pointer(handle), cID, cContext, cBool(validateContext))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativePackageFree(handle nativeHandle) {
	if handle == 0 || ensureNative() != nil {
		return
	}
	C.rototo_go_call_handle_free(native.packageFree, pointer(handle))
}

func nativeRefreshingPackageLoad(source string, periodSeconds *float64, packageToken, lint string) (nativeHandle, error) {
	if err := ensureNative(); err != nil {
		return 0, err
	}
	cSource := C.CString(source)
	cLint := C.CString(lint)
	defer C.free(unsafe.Pointer(cSource))
	defer C.free(unsafe.Pointer(cLint))
	cToken, freeToken := optionalCString(packageToken)
	defer freeToken()
	var seconds C.double
	var hasSeconds C.int
	if periodSeconds != nil {
		seconds = C.double(*periodSeconds)
		hasSeconds = 1
	}
	result := C.rototo_go_call_refreshing_package_load(
		native.refreshingPackageLoad,
		cSource,
		seconds,
		hasSeconds,
		cToken,
		cLint,
	)
	defer C.rototo_go_call_handle_result_free(native.handleResultFree, &result)
	return handleResult(result)
}

func nativeRefreshingPackageResolveVariable(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	return nativeRefreshingPackageResolve(native.refreshingPackageResolveVariable, handle, id, contextJSON, validateContext)
}

func nativeRefreshingPackageResolveQualifier(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	return nativeRefreshingPackageResolve(native.refreshingPackageResolveQualifier, handle, id, contextJSON, validateContext)
}

func nativeRefreshingPackageResolve(fn unsafe.Pointer, handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	cID := C.CString(id)
	cContext := C.CString(contextJSON)
	defer C.free(unsafe.Pointer(cID))
	defer C.free(unsafe.Pointer(cContext))
	result := C.rototo_go_call_refreshing_package_resolve(fn, pointer(handle), cID, cContext, cBool(validateContext))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeRefreshingPackageRefreshNow(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_refreshing_package_string(native.refreshingPackageRefreshNow, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeRefreshingPackageStatus(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_refreshing_package_string(native.refreshingPackageStatus, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeRefreshingPackageIdentity(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_refreshing_package_string(native.refreshingPackageIdentity, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeRefreshingPackageSnapshot(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_refreshing_package_string(native.refreshingPackageSnapshot, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeRefreshingPackageSubscribeEvents(handle nativeHandle) (nativeHandle, error) {
	if err := ensureNative(); err != nil {
		return 0, err
	}
	result := C.rototo_go_call_subscribe(native.refreshingPackageSubscribeEvents, pointer(handle))
	defer C.rototo_go_call_handle_result_free(native.handleResultFree, &result)
	return handleResult(result)
}

// nativeRefreshEventsNext blocks until the next event. The bool is false (with a
// nil error) when the stream has closed.
func nativeRefreshEventsNext(handle nativeHandle) (string, bool, error) {
	if err := ensureNative(); err != nil {
		return "", false, err
	}
	result := C.rototo_go_call_refreshing_package_string(native.refreshEventsNext, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	if result.error != nil {
		return "", false, nativeError(C.GoString(result.error))
	}
	if result.value == nil {
		return "", false, nil
	}
	return C.GoString(result.value), true, nil
}

func nativeRefreshEventsFree(handle nativeHandle) {
	if handle == 0 || ensureNative() != nil {
		return
	}
	C.rototo_go_call_handle_free(native.refreshEventsFree, pointer(handle))
}

func nativeRefreshingPackageSubscribeTraceEvents(handle nativeHandle) (nativeHandle, error) {
	if err := ensureNative(); err != nil {
		return 0, err
	}
	result := C.rototo_go_call_subscribe(native.refreshingPackageSubscribeTrace, pointer(handle))
	defer C.rototo_go_call_handle_result_free(native.handleResultFree, &result)
	return handleResult(result)
}

// nativeTraceEventsNext blocks until the next trace stream item. The bool is
// false (with a nil error) when the stream has closed.
func nativeTraceEventsNext(handle nativeHandle) (string, bool, error) {
	if err := ensureNative(); err != nil {
		return "", false, err
	}
	result := C.rototo_go_call_refreshing_package_string(native.traceEventsNext, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	if result.error != nil {
		return "", false, nativeError(C.GoString(result.error))
	}
	if result.value == nil {
		return "", false, nil
	}
	return C.GoString(result.value), true, nil
}

func nativeTraceEventsFree(handle nativeHandle) {
	if handle == 0 || ensureNative() != nil {
		return
	}
	C.rototo_go_call_handle_free(native.traceEventsFree, pointer(handle))
}

func nativeRefreshingPackageShutdown(handle nativeHandle) error {
	if err := ensureNative(); err != nil {
		return err
	}
	result := C.rototo_go_call_refreshing_package_void(native.refreshingPackageShutdown, pointer(handle))
	defer C.rototo_go_call_void_result_free(native.voidResultFree, &result)
	return voidResult(result)
}

func nativeRefreshingPackageFree(handle nativeHandle) {
	if handle == 0 || ensureNative() != nil {
		return
	}
	C.rototo_go_call_handle_free(native.refreshingPackageFree, pointer(handle))
}

func optionalCString(value string) (*C.char, func()) {
	if value == "" {
		return nil, func() {}
	}
	cValue := C.CString(value)
	return cValue, func() { C.free(unsafe.Pointer(cValue)) }
}

func stringResult(result C.RototoGoStringResult) (string, error) {
	if result.error != nil {
		return "", nativeError(C.GoString(result.error))
	}
	return C.GoString(result.value), nil
}

func handleResult(result C.RototoGoHandleResult) (nativeHandle, error) {
	if result.error != nil {
		return 0, nativeError(C.GoString(result.error))
	}
	if result.handle == nil {
		return 0, nativeError("rototo Go native call returned nil handle")
	}
	return nativeHandle(uintptr(result.handle)), nil
}

func voidResult(result C.RototoGoVoidResult) error {
	if result.error != nil {
		return nativeError(C.GoString(result.error))
	}
	return nil
}

func pointer(handle nativeHandle) unsafe.Pointer {
	return unsafe.Pointer(uintptr(handle))
}

func cBool(value bool) C.int {
	if value {
		return 1
	}
	return 0
}
