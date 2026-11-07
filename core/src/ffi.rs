//! The C interface the Qt shell links against (declared in src/nimrag.h).
//!
//! Everything crosses as UTF-8 JSON in strings the core allocates; the shell
//! hands each one back to `nimrag_string_free`. JSON keeps the boundary narrow
//! and is the same text the command line prints with `--json`.

use std::ffi::{CStr, CString, c_char, c_int};
use std::path::Path;

use crate::{cli, device, status};

fn to_c(text: String) -> *mut c_char {
    // JSON never holds a NUL byte; should one appear, give back an empty string.
    CString::new(text).unwrap_or_default().into_raw()
}

/// The connected inReach devices as a JSON array of `status::DeviceStatus`.
/// With `ask` non-zero each device is asked for its mode and version, which
/// can take seconds: call it off the UI thread.
#[unsafe(no_mangle)]
pub extern "C" fn nimrag_devices_json(ask: c_int) -> *mut c_char {
    let list = status::devices(Path::new(device::SYSFS_HIDRAW), ask != 0);
    to_c(serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()))
}

/// Restarts the inReach at `node` as a USB drive. Returns JSON:
/// `{"switched": true|false}` or `{"error": "…"}`.
///
/// # Safety
/// `node` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nimrag_switch_to_drive(node: *const c_char) -> *mut c_char {
    if node.is_null() {
        return to_c(r#"{"error":"no device given"}"#.into());
    }
    // SAFETY: the caller passes a valid C string.
    let node = unsafe { CStr::from_ptr(node) }.to_string_lossy().into_owned();
    let answer = match status::switch_to_drive(Path::new(device::SYSFS_HIDRAW), &node) {
        Ok(switched) => serde_json::json!({ "switched": switched }),
        Err(e) => serde_json::json!({ "error": e }),
    };
    to_c(answer.to_string())
}

/// The command line: `argv` as `main` got it. Returns the exit code.
///
/// # Safety
/// `argv` must hold `argc` valid NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nimrag_cli_main(argc: c_int, argv: *const *const c_char) -> c_int {
    let args: Vec<String> = (0..argc.max(0) as usize)
        // SAFETY: the caller passes argc valid C strings.
        .map(|i| unsafe { CStr::from_ptr(*argv.add(i)) }.to_string_lossy().into_owned())
        .collect();
    cli::run(&args, &mut std::io::stdout(), &mut std::io::stderr())
}

/// Frees a string this library returned.
///
/// # Safety
/// `text` must come from this library and not be freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nimrag_string_free(text: *mut c_char) {
    if !text.is_null() {
        // SAFETY: allocated by CString::into_raw above.
        drop(unsafe { CString::from_raw(text) });
    }
}
