//! FFI Layer - Multi-language SDK Support
//!
//! This module provides a C-compatible Foreign Function Interface (FFI) that
//! enables the web server abstraction to be used from other programming languages.

use crate::types::HttpMethod;

use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    os::raw::{c_char, c_int},
    ptr,
    sync::{Mutex, OnceLock},
};

/// Global FFI context with thread-safe access
static FFI_CONTEXT: OnceLock<Mutex<FfiContext>> = OnceLock::new();

/// FFI context for managing server instances and state
pub struct FfiContext {
    servers: HashMap<u32, Box<dyn FfiServer>>,
    next_id: u32,
    last_error: Option<String>,
}

impl FfiContext {
    fn new() -> Self {
        Self {
            servers: HashMap::new(),
            next_id: 1,
            last_error: None,
        }
    }

    fn add_server(&mut self, server: Box<dyn FfiServer>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.servers.insert(id, server);
        id
    }

    fn get_server(&mut self, id: u32) -> Option<&mut Box<dyn FfiServer>> {
        self.servers.get_mut(&id)
    }

    fn remove_server(&mut self, id: u32) -> Option<Box<dyn FfiServer>> {
        self.servers.remove(&id)
    }

    fn set_error(&mut self, error: String) {
        self.last_error = Some(error);
    }

    fn get_last_error(&self) -> Option<&String> {
        self.last_error.as_ref()
    }
}

/// FFI-compatible server trait
trait FfiServer: Send + Sync {
    fn add_route(&mut self, path: &str, method: HttpMethod, handler_id: u32) -> Result<(), String>;
    fn bind(&mut self, addr: &str) -> Result<(), String>;
    fn start(&mut self) -> Result<(), String>;
    fn stop(&mut self) -> Result<(), String>;
    fn is_running(&self) -> bool;
}

/// Mock FFI server for testing
struct MockFfiServer {
    routes: Vec<(String, HttpMethod, u32)>,
    address: Option<String>,
    running: bool,
}

impl MockFfiServer {
    fn new() -> Self {
        Self {
            routes: Vec::new(),
            address: None,
            running: false,
        }
    }
}

impl FfiServer for MockFfiServer {
    fn add_route(&mut self, path: &str, method: HttpMethod, handler_id: u32) -> Result<(), String> {
        self.routes.push((path.to_string(), method, handler_id));
        Ok(())
    }

    fn bind(&mut self, addr: &str) -> Result<(), String> {
        self.address = Some(addr.to_string());
        Ok(())
    }

    fn start(&mut self) -> Result<(), String> {
        if self.address.is_none() {
            return Err("Server not bound to an address".to_string());
        }
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }
}

/// Get mutable reference to FFI context
fn get_ffi_context() -> std::sync::MutexGuard<'static, FfiContext> {
    FFI_CONTEXT
        .get_or_init(|| Mutex::new(FfiContext::new()))
        .lock()
        .unwrap()
}

/// Convert C string to Rust string
unsafe fn c_str_to_string(c_str: *const c_char) -> Result<String, String> {
    if c_str.is_null() {
        return Err("Null pointer provided".to_string());
    }

    unsafe {
        match CStr::from_ptr(c_str).to_str() {
            Ok(s) => Ok(s.to_string()),
            Err(_) => Err("Invalid UTF-8 string".to_string()),
        }
    }
}

/// Convert Rust string to C string (caller must free)
fn string_to_c_str(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(c_string) => c_string.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// Convert HttpMethod enum to C int
#[allow(dead_code)]
fn http_method_to_c_int(method: HttpMethod) -> c_int {
    match method {
        HttpMethod::GET => 0,
        HttpMethod::POST => 1,
        HttpMethod::PUT => 2,
        HttpMethod::DELETE => 3,
        HttpMethod::PATCH => 4,
        HttpMethod::HEAD => 5,
        HttpMethod::OPTIONS => 6,
        HttpMethod::TRACE => 7,
        HttpMethod::CONNECT => 8,
    }
}

/// Convert C int to HttpMethod enum
fn c_int_to_http_method(method: c_int) -> Result<HttpMethod, String> {
    match method {
        0 => Ok(HttpMethod::GET),
        1 => Ok(HttpMethod::POST),
        2 => Ok(HttpMethod::PUT),
        3 => Ok(HttpMethod::DELETE),
        4 => Ok(HttpMethod::PATCH),
        5 => Ok(HttpMethod::HEAD),
        6 => Ok(HttpMethod::OPTIONS),
        7 => Ok(HttpMethod::TRACE),
        8 => Ok(HttpMethod::CONNECT),
        _ => Err(format!("Invalid HTTP method: {}", method)),
    }
}

/// C-compatible result codes
pub const FFI_SUCCESS: c_int = 0;
pub const FFI_ERROR_NULL_POINTER: c_int = -1;
pub const FFI_ERROR_INVALID_ARGUMENT: c_int = -2;
pub const FFI_ERROR_SERVER_NOT_FOUND: c_int = -3;
pub const FFI_ERROR_INTERNAL: c_int = -4;

// ================================
// FFI FUNCTIONS - C INTERFACE
// ================================

/// Initialize the FFI library
/// Returns FFI_SUCCESS on success
#[unsafe(no_mangle)]
pub extern "C" fn ws_ffi_init() -> c_int {
    // Context is automatically initialized on first access
    FFI_SUCCESS
}

/// Create a new web server instance
/// Returns server ID on success, negative value on error
#[unsafe(no_mangle)]
pub extern "C" fn ws_create_server() -> c_int {
    let mut context = get_ffi_context();
    let server = Box::new(MockFfiServer::new());
    let id = context.add_server(server);
    id as c_int
}

/// Bind server to an address
/// Returns FFI_SUCCESS on success, negative value on error
///
/// # Safety
///
/// `address` must be a valid, NUL-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ws_bind_server(server_id: c_int, address: *const c_char) -> c_int {
    let mut context = get_ffi_context();

    let addr = match unsafe { c_str_to_string(address) } {
        Ok(s) => s,
        Err(e) => {
            context.set_error(e);
            return FFI_ERROR_INVALID_ARGUMENT;
        }
    };

    match context.get_server(server_id as u32) {
        Some(server) => match server.bind(&addr) {
            Ok(()) => FFI_SUCCESS,
            Err(e) => {
                context.set_error(e);
                FFI_ERROR_INTERNAL
            }
        },
        None => {
            context.set_error("Server not found".to_string());
            FFI_ERROR_SERVER_NOT_FOUND
        }
    }
}

/// Add a route to the server
/// Returns FFI_SUCCESS on success, negative value on error
///
/// # Safety
///
/// Every pointer argument must be a valid, NUL-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ws_add_route(
    server_id: c_int,
    path: *const c_char,
    method: c_int,
    handler_id: c_int,
) -> c_int {
    let mut context = get_ffi_context();

    let path_str = match unsafe { c_str_to_string(path) } {
        Ok(s) => s,
        Err(e) => {
            context.set_error(e);
            return FFI_ERROR_INVALID_ARGUMENT;
        }
    };

    let http_method = match c_int_to_http_method(method) {
        Ok(m) => m,
        Err(e) => {
            context.set_error(e);
            return FFI_ERROR_INVALID_ARGUMENT;
        }
    };

    match context.get_server(server_id as u32) {
        Some(server) => match server.add_route(&path_str, http_method, handler_id as u32) {
            Ok(()) => FFI_SUCCESS,
            Err(e) => {
                context.set_error(e);
                FFI_ERROR_INTERNAL
            }
        },
        None => {
            context.set_error("Server not found".to_string());
            FFI_ERROR_SERVER_NOT_FOUND
        }
    }
}

/// Start the server
/// Returns FFI_SUCCESS on success, negative value on error
#[unsafe(no_mangle)]
pub extern "C" fn ws_start_server(server_id: c_int) -> c_int {
    let mut context = get_ffi_context();

    match context.get_server(server_id as u32) {
        Some(server) => match server.start() {
            Ok(()) => FFI_SUCCESS,
            Err(e) => {
                context.set_error(e);
                FFI_ERROR_INTERNAL
            }
        },
        None => {
            context.set_error("Server not found".to_string());
            FFI_ERROR_SERVER_NOT_FOUND
        }
    }
}

/// Stop the server
/// Returns FFI_SUCCESS on success, negative value on error
#[unsafe(no_mangle)]
pub extern "C" fn ws_stop_server(server_id: c_int) -> c_int {
    let mut context = get_ffi_context();

    match context.get_server(server_id as u32) {
        Some(server) => match server.stop() {
            Ok(()) => FFI_SUCCESS,
            Err(e) => {
                context.set_error(e);
                FFI_ERROR_INTERNAL
            }
        },
        None => {
            context.set_error("Server not found".to_string());
            FFI_ERROR_SERVER_NOT_FOUND
        }
    }
}

/// Check if server is running
/// Returns 1 if running, 0 if not running, negative value on error
#[unsafe(no_mangle)]
pub extern "C" fn ws_is_server_running(server_id: c_int) -> c_int {
    let mut context = get_ffi_context();

    match context.get_server(server_id as u32) {
        Some(server) => {
            if server.is_running() {
                1
            } else {
                0
            }
        }
        None => {
            context.set_error("Server not found".to_string());
            FFI_ERROR_SERVER_NOT_FOUND
        }
    }
}

/// Destroy a server instance
/// Returns FFI_SUCCESS on success, negative value on error
#[unsafe(no_mangle)]
pub extern "C" fn ws_destroy_server(server_id: c_int) -> c_int {
    let mut context = get_ffi_context();

    match context.remove_server(server_id as u32) {
        Some(_) => FFI_SUCCESS,
        None => {
            context.set_error("Server not found".to_string());
            FFI_ERROR_SERVER_NOT_FOUND
        }
    }
}

/// Get the last error message
/// Returns pointer to error string (caller must free with ws_free_string)
/// Returns null if no error
#[unsafe(no_mangle)]
pub extern "C" fn ws_get_last_error() -> *mut c_char {
    let context = get_ffi_context();

    match context.get_last_error() {
        Some(error) => string_to_c_str(error),
        None => ptr::null_mut(),
    }
}

/// Free a string allocated by the FFI
///
/// # Safety
///
/// `s` must be null or a pointer returned by this library that has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ws_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

/// Get library version
/// Returns pointer to version string (caller must free with ws_free_string)
#[unsafe(no_mangle)]
pub extern "C" fn ws_get_version() -> *mut c_char {
    string_to_c_str(env!("CARGO_PKG_VERSION"))
}

/// Cleanup the FFI library (call at program exit)
#[unsafe(no_mangle)]
pub extern "C" fn ws_ffi_cleanup() -> c_int {
    // Note: In a real implementation, you'd clean up any remaining resources
    FFI_SUCCESS
}

// ================================
// LANGUAGE-SPECIFIC HELPERS
// ================================

/// Python bindings helper structures
#[repr(C)]
pub struct PythonServerHandle {
    pub server_id: c_int,
    pub is_valid: c_int,
}

/// Create Python-compatible server handle
#[unsafe(no_mangle)]
pub extern "C" fn ws_create_python_server() -> PythonServerHandle {
    let server_id = ws_create_server();
    PythonServerHandle {
        server_id,
        is_valid: if server_id >= 0 { 1 } else { 0 },
    }
}

/// Node.js bindings helper - create server with callback support
#[unsafe(no_mangle)]
pub extern "C" fn ws_create_nodejs_server(
    _callback: extern "C" fn(*const c_char, c_int, *const c_char) -> c_int,
) -> c_int {
    // In a real implementation, you'd store the callback for later use
    ws_create_server()
}

/// Go bindings helper - create server with Go-style error handling
///
/// # Safety
///
/// `error_out` must be null or point to writable memory for one pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ws_create_go_server(error_out: *mut *mut c_char) -> c_int {
    let server_id = ws_create_server();

    if server_id < 0 {
        let error = ws_get_last_error();
        if !error.is_null() {
            unsafe {
                *error_out = error;
            }
        }
    } else {
        unsafe {
            *error_out = ptr::null_mut();
        }
    }

    server_id
}

// ================================
// HEADER GENERATION HELPER
// ================================

/// Generate C header file content for FFI bindings
pub fn generate_c_header() -> String {
    r#"
#ifndef WEB_SERVER_ABSTRACTION_FFI_H
#define WEB_SERVER_ABSTRACTION_FFI_H

#ifdef __cplusplus
extern "C" {
#endif

// Result codes
#define FFI_SUCCESS 0
#define FFI_ERROR_NULL_POINTER -1
#define FFI_ERROR_INVALID_ARGUMENT -2
#define FFI_ERROR_SERVER_NOT_FOUND -3
#define FFI_ERROR_INTERNAL -4

// HTTP methods
#define HTTP_GET 0
#define HTTP_POST 1
#define HTTP_PUT 2
#define HTTP_DELETE 3
#define HTTP_PATCH 4
#define HTTP_HEAD 5
#define HTTP_OPTIONS 6
#define HTTP_TRACE 7
#define HTTP_CONNECT 8

// Core functions
int ws_ffi_init(void);
int ws_create_server(void);
int ws_bind_server(int server_id, const char* address);
int ws_add_route(int server_id, const char* path, int method, int handler_id);
int ws_start_server(int server_id);
int ws_stop_server(int server_id);
int ws_is_server_running(int server_id);
int ws_destroy_server(int server_id);
char* ws_get_last_error(void);
void ws_free_string(char* s);
char* ws_get_version(void);
int ws_ffi_cleanup(void);

// Language-specific helpers
typedef struct {
    int server_id;
    int is_valid;
} PythonServerHandle;

PythonServerHandle ws_create_python_server(void);
int ws_create_nodejs_server(int (*callback)(const char*, int, const char*));
int ws_create_go_server(char** error_out);

#ifdef __cplusplus
}
#endif

#endif // WEB_SERVER_ABSTRACTION_FFI_H
"#
    .to_string()
}

/// Save C header to file
pub fn save_c_header(path: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, generate_c_header())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_server_lifecycle() {
        // Initialize FFI
        assert_eq!(ws_ffi_init(), FFI_SUCCESS);

        // Create server
        let server_id = ws_create_server();
        assert!(server_id >= 0);

        // Bind server
        let addr = CString::new("127.0.0.1:8080").unwrap();
        assert_eq!(
            unsafe { ws_bind_server(server_id, addr.as_ptr()) },
            FFI_SUCCESS
        );

        // Add route
        let path = CString::new("/test").unwrap();
        assert_eq!(
            unsafe { ws_add_route(server_id, path.as_ptr(), 0, 1) },
            FFI_SUCCESS
        );

        // Start server
        assert_eq!(ws_start_server(server_id), FFI_SUCCESS);
        assert_eq!(ws_is_server_running(server_id), 1);

        // Stop server
        assert_eq!(ws_stop_server(server_id), FFI_SUCCESS);
        assert_eq!(ws_is_server_running(server_id), 0);

        // Destroy server
        assert_eq!(ws_destroy_server(server_id), FFI_SUCCESS);

        // Cleanup
        assert_eq!(ws_ffi_cleanup(), FFI_SUCCESS);
    }

    #[test]
    fn test_ffi_error_handling() {
        ws_ffi_init();

        // Test invalid server ID
        assert_eq!(ws_start_server(999), FFI_ERROR_SERVER_NOT_FOUND);

        // Get error message
        let error = ws_get_last_error();
        assert!(!error.is_null());

        // Free error string
        unsafe { ws_free_string(error) };
    }

    #[test]
    fn test_ffi_version() {
        let version = ws_get_version();
        assert!(!version.is_null());
        unsafe { ws_free_string(version) };
    }

    #[test]
    fn test_python_bindings() {
        ws_ffi_init();
        let handle = ws_create_python_server();
        assert_eq!(handle.is_valid, 1);
        assert!(handle.server_id >= 0);
        ws_destroy_server(handle.server_id);
    }

    #[test]
    fn test_c_header_generation() {
        let header = generate_c_header();
        assert!(header.contains("#ifndef WEB_SERVER_ABSTRACTION_FFI_H"));
        assert!(header.contains("int ws_create_server(void);"));
        assert!(header.contains("#define FFI_SUCCESS 0"));
    }
}
