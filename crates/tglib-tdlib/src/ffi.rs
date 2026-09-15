//! Minimal tdjson C API. TDLib JSON types stay inside this crate.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_void};

#[link(name = "tdjson")]
unsafe extern "C" {
    fn td_json_client_create() -> *mut c_void;
    fn td_json_client_send(client: *mut c_void, request: *const c_char);
    fn td_json_client_receive(client: *mut c_void, timeout: c_double) -> *const c_char;
    fn td_json_client_execute(client: *mut c_void, request: *const c_char) -> *const c_char;
    fn td_json_client_destroy(client: *mut c_void);
}

#[derive(Debug)]
pub struct TdjsonClient {
    ptr: *mut c_void,
}

unsafe impl Send for TdjsonClient {}
unsafe impl Sync for TdjsonClient {}

impl TdjsonClient {
    pub fn create() -> Option<Self> {
        let ptr = unsafe { td_json_client_create() };
        if ptr.is_null() {
            None
        } else {
            Some(Self { ptr })
        }
    }

    pub fn send_json(&self, request: &str) {
        let c = CString::new(request).unwrap_or_else(|_| CString::new("{}").expect("empty"));
        unsafe { td_json_client_send(self.ptr, c.as_ptr()) };
    }

    /// Copy the returned JSON immediately; the pointer is invalid after the next receive/execute.
    pub fn receive(&self, timeout_secs: f64) -> Option<String> {
        let ptr = unsafe { td_json_client_receive(self.ptr, timeout_secs) };
        if ptr.is_null() {
            return None;
        }
        unsafe { CStr::from_ptr(ptr) }.to_str().ok().map(str::to_string)
    }

    pub fn execute_json(request: &str) -> Option<String> {
        let c = CString::new(request).ok()?;
        let ptr = unsafe { td_json_client_execute(std::ptr::null_mut(), c.as_ptr()) };
        if ptr.is_null() {
            return None;
        }
        unsafe { CStr::from_ptr(ptr) }.to_str().ok().map(str::to_string)
    }
}

impl Drop for TdjsonClient {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { td_json_client_destroy(self.ptr) };
            self.ptr = std::ptr::null_mut();
        }
    }
}
