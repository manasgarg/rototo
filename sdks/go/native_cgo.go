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
typedef RototoGoHandleResult (*rototo_go_workspace_load_fn)(const char*, const char*, const char*);
typedef RototoGoHandleResult (*rototo_go_workspace_inspect_fn)(const char*, const char*);
typedef RototoGoStringResult (*rototo_go_workspace_string_fn)(void*);
typedef RototoGoStringResult (*rototo_go_workspace_resolve_fn)(void*, const char*, const char*, int);
typedef void (*rototo_go_handle_free_fn)(void*);
typedef RototoGoHandleResult (*rototo_go_refreshing_workspace_load_fn)(const char*, double, int, const char*, const char*);
typedef RototoGoStringResult (*rototo_go_refreshing_workspace_string_fn)(void*);
typedef RototoGoStringResult (*rototo_go_refreshing_workspace_resolve_fn)(void*, const char*, const char*, int);
typedef RototoGoVoidResult (*rototo_go_refreshing_workspace_void_fn)(void*);
typedef void (*rototo_go_string_result_free_fn)(RototoGoStringResult*);
typedef void (*rototo_go_handle_result_free_fn)(RototoGoHandleResult*);
typedef void (*rototo_go_void_result_free_fn)(RototoGoVoidResult*);

static RototoGoStringResult rototo_go_call_version(void* fn) {
    return ((rototo_go_version_fn)fn)();
}
static RototoGoHandleResult rototo_go_call_workspace_load(void* fn, const char* source, const char* token, const char* lint) {
    return ((rototo_go_workspace_load_fn)fn)(source, token, lint);
}
static RototoGoHandleResult rototo_go_call_workspace_inspect(void* fn, const char* source, const char* token) {
    return ((rototo_go_workspace_inspect_fn)fn)(source, token);
}
static RototoGoStringResult rototo_go_call_workspace_string(void* fn, void* handle) {
    return ((rototo_go_workspace_string_fn)fn)(handle);
}
static RototoGoStringResult rototo_go_call_workspace_resolve(void* fn, void* handle, const char* id, const char* context, int validate_context) {
    return ((rototo_go_workspace_resolve_fn)fn)(handle, id, context, validate_context);
}
static void rototo_go_call_handle_free(void* fn, void* handle) {
    ((rototo_go_handle_free_fn)fn)(handle);
}
static RototoGoHandleResult rototo_go_call_refreshing_workspace_load(void* fn, const char* source, double period_seconds, int has_period_seconds, const char* token, const char* lint) {
    return ((rototo_go_refreshing_workspace_load_fn)fn)(source, period_seconds, has_period_seconds, token, lint);
}
static RototoGoStringResult rototo_go_call_refreshing_workspace_string(void* fn, void* handle) {
    return ((rototo_go_refreshing_workspace_string_fn)fn)(handle);
}
static RototoGoStringResult rototo_go_call_refreshing_workspace_resolve(void* fn, void* handle, const char* id, const char* context, int validate_context) {
    return ((rototo_go_refreshing_workspace_resolve_fn)fn)(handle, id, context, validate_context);
}
static RototoGoVoidResult rototo_go_call_refreshing_workspace_void(void* fn, void* handle) {
    return ((rototo_go_refreshing_workspace_void_fn)fn)(handle);
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
	version                             unsafe.Pointer
	workspaceLoad                       unsafe.Pointer
	workspaceInspect                    unsafe.Pointer
	workspaceRoot                       unsafe.Pointer
	workspaceLint                       unsafe.Pointer
	workspaceResolveVariable            unsafe.Pointer
	workspaceResolveQualifier           unsafe.Pointer
	workspaceFree                       unsafe.Pointer
	refreshingWorkspaceLoad             unsafe.Pointer
	refreshingWorkspaceResolveVariable  unsafe.Pointer
	refreshingWorkspaceResolveQualifier unsafe.Pointer
	refreshingWorkspaceRefreshNow       unsafe.Pointer
	refreshingWorkspaceStatus           unsafe.Pointer
	refreshingWorkspaceShutdown         unsafe.Pointer
	refreshingWorkspaceFree             unsafe.Pointer
	stringResultFree                    unsafe.Pointer
	handleResultFree                    unsafe.Pointer
	voidResultFree                      unsafe.Pointer
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
	native.workspaceLoad = symbol("rototo_go_workspace_load")
	native.workspaceInspect = symbol("rototo_go_workspace_inspect")
	native.workspaceRoot = symbol("rototo_go_workspace_root")
	native.workspaceLint = symbol("rototo_go_workspace_lint")
	native.workspaceResolveVariable = symbol("rototo_go_workspace_resolve_variable")
	native.workspaceResolveQualifier = symbol("rototo_go_workspace_resolve_qualifier")
	native.workspaceFree = symbol("rototo_go_workspace_free")
	native.refreshingWorkspaceLoad = symbol("rototo_go_refreshing_workspace_load")
	native.refreshingWorkspaceResolveVariable = symbol("rototo_go_refreshing_workspace_resolve_variable")
	native.refreshingWorkspaceResolveQualifier = symbol("rototo_go_refreshing_workspace_resolve_qualifier")
	native.refreshingWorkspaceRefreshNow = symbol("rototo_go_refreshing_workspace_refresh_now")
	native.refreshingWorkspaceStatus = symbol("rototo_go_refreshing_workspace_status")
	native.refreshingWorkspaceShutdown = symbol("rototo_go_refreshing_workspace_shutdown")
	native.refreshingWorkspaceFree = symbol("rototo_go_refreshing_workspace_free")
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

func nativeWorkspaceLoad(source, workspaceToken, lint string) (nativeHandle, error) {
	if err := ensureNative(); err != nil {
		return 0, err
	}
	cSource := C.CString(source)
	cLint := C.CString(lint)
	defer C.free(unsafe.Pointer(cSource))
	defer C.free(unsafe.Pointer(cLint))
	cToken, freeToken := optionalCString(workspaceToken)
	defer freeToken()
	result := C.rototo_go_call_workspace_load(native.workspaceLoad, cSource, cToken, cLint)
	defer C.rototo_go_call_handle_result_free(native.handleResultFree, &result)
	return handleResult(result)
}

func nativeWorkspaceInspect(source, workspaceToken string) (nativeHandle, error) {
	if err := ensureNative(); err != nil {
		return 0, err
	}
	cSource := C.CString(source)
	defer C.free(unsafe.Pointer(cSource))
	cToken, freeToken := optionalCString(workspaceToken)
	defer freeToken()
	result := C.rototo_go_call_workspace_inspect(native.workspaceInspect, cSource, cToken)
	defer C.rototo_go_call_handle_result_free(native.handleResultFree, &result)
	return handleResult(result)
}

func nativeWorkspaceRoot(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_workspace_string(native.workspaceRoot, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeWorkspaceLint(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_workspace_string(native.workspaceLint, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeWorkspaceResolveVariable(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	return nativeWorkspaceResolve(native.workspaceResolveVariable, handle, id, contextJSON, validateContext)
}

func nativeWorkspaceResolveQualifier(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	return nativeWorkspaceResolve(native.workspaceResolveQualifier, handle, id, contextJSON, validateContext)
}

func nativeWorkspaceResolve(fn unsafe.Pointer, handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	cID := C.CString(id)
	cContext := C.CString(contextJSON)
	defer C.free(unsafe.Pointer(cID))
	defer C.free(unsafe.Pointer(cContext))
	result := C.rototo_go_call_workspace_resolve(fn, pointer(handle), cID, cContext, cBool(validateContext))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeWorkspaceFree(handle nativeHandle) {
	if handle == 0 || ensureNative() != nil {
		return
	}
	C.rototo_go_call_handle_free(native.workspaceFree, pointer(handle))
}

func nativeRefreshingWorkspaceLoad(source string, periodSeconds *float64, workspaceToken, lint string) (nativeHandle, error) {
	if err := ensureNative(); err != nil {
		return 0, err
	}
	cSource := C.CString(source)
	cLint := C.CString(lint)
	defer C.free(unsafe.Pointer(cSource))
	defer C.free(unsafe.Pointer(cLint))
	cToken, freeToken := optionalCString(workspaceToken)
	defer freeToken()
	var seconds C.double
	var hasSeconds C.int
	if periodSeconds != nil {
		seconds = C.double(*periodSeconds)
		hasSeconds = 1
	}
	result := C.rototo_go_call_refreshing_workspace_load(
		native.refreshingWorkspaceLoad,
		cSource,
		seconds,
		hasSeconds,
		cToken,
		cLint,
	)
	defer C.rototo_go_call_handle_result_free(native.handleResultFree, &result)
	return handleResult(result)
}

func nativeRefreshingWorkspaceResolveVariable(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	return nativeRefreshingWorkspaceResolve(native.refreshingWorkspaceResolveVariable, handle, id, contextJSON, validateContext)
}

func nativeRefreshingWorkspaceResolveQualifier(handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	return nativeRefreshingWorkspaceResolve(native.refreshingWorkspaceResolveQualifier, handle, id, contextJSON, validateContext)
}

func nativeRefreshingWorkspaceResolve(fn unsafe.Pointer, handle nativeHandle, id, contextJSON string, validateContext bool) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	cID := C.CString(id)
	cContext := C.CString(contextJSON)
	defer C.free(unsafe.Pointer(cID))
	defer C.free(unsafe.Pointer(cContext))
	result := C.rototo_go_call_refreshing_workspace_resolve(fn, pointer(handle), cID, cContext, cBool(validateContext))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeRefreshingWorkspaceRefreshNow(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_refreshing_workspace_string(native.refreshingWorkspaceRefreshNow, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeRefreshingWorkspaceStatus(handle nativeHandle) (string, error) {
	if err := ensureNative(); err != nil {
		return "", err
	}
	result := C.rototo_go_call_refreshing_workspace_string(native.refreshingWorkspaceStatus, pointer(handle))
	defer C.rototo_go_call_string_result_free(native.stringResultFree, &result)
	return stringResult(result)
}

func nativeRefreshingWorkspaceShutdown(handle nativeHandle) error {
	if err := ensureNative(); err != nil {
		return err
	}
	result := C.rototo_go_call_refreshing_workspace_void(native.refreshingWorkspaceShutdown, pointer(handle))
	defer C.rototo_go_call_void_result_free(native.voidResultFree, &result)
	return voidResult(result)
}

func nativeRefreshingWorkspaceFree(handle nativeHandle) {
	if handle == 0 || ensureNative() != nil {
		return
	}
	C.rototo_go_call_handle_free(native.refreshingWorkspaceFree, pointer(handle))
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
