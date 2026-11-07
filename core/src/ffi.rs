//! The C interface the Qt shell links against (declared in src/nimrag.h).
//!
//! Everything crosses as UTF-8 JSON in strings the core allocates; the shell
//! hands each one back to `nimrag_string_free`. JSON keeps the boundary narrow
//! and is the same text the command line prints with `--json`.

use std::ffi::{CStr, CString, c_char, c_int};
use std::path::Path;

use crate::{cli, credentials, device, status, sync};

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

fn arg(text: *const c_char) -> Option<String> {
    // SAFETY: the callers pass valid C strings or null.
    (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned())
}

fn answer(result: Result<serde_json::Value, String>) -> *mut c_char {
    to_c(match result {
        Ok(v) => v.to_string(),
        Err(e) => serde_json::json!({ "error": e }).to_string(),
    })
}

/// The signed-in account: `{"email": "…"}`, or `{"email": null}`.
#[unsafe(no_mangle)]
pub extern "C" fn nimrag_account_json() -> *mut c_char {
    to_c(serde_json::json!({ "email": credentials::signed_in_email(&credentials::config_dir()) }).to_string())
}

/// Signs in: the password to the keyring. `{"email": "…"}` or `{"error": "…"}`.
///
/// # Safety
/// Both arguments must be valid NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nimrag_sign_in(email: *const c_char, password: *const c_char) -> *mut c_char {
    let (email, password) = (arg(email).unwrap_or_default(), arg(password).unwrap_or_default());
    answer(
        credentials::sign_in(&credentials::config_dir(), &credentials::Keyring::default(), &email, &password)
            .map(|()| serde_json::json!({ "email": email.trim() }))
            .map_err(|e| e.to_string()),
    )
}

/// Signs out: `{"email": "…" or null}` or `{"error": "…"}`.
#[unsafe(no_mangle)]
pub extern "C" fn nimrag_sign_out() -> *mut c_char {
    answer(
        credentials::sign_out(&credentials::config_dir(), &credentials::Keyring::default())
            .map(|email| serde_json::json!({ "email": email }))
            .map_err(|e| e.to_string()),
    )
}

/// Restarts the inReach at `node` in app mode (it switches on): `{"restarted": bool}` or `{"error": "…"}`.
///
/// # Safety
/// `node` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nimrag_switch_on(node: *const c_char) -> *mut c_char {
    let node = arg(node).unwrap_or_default();
    answer((|| {
        let found = device::find(Path::new(device::SYSFS_HIDRAW))
            .into_iter()
            .find(|f| f.node == Path::new(&node))
            .ok_or_else(|| format!("no inReach at {node}"))?;
        let mut dev = device::InReach::open(&found).map_err(|e| e.to_string())?;
        if dev.app_mode().map_err(|e| e.to_string())? == device::AppMode::App {
            return Ok(serde_json::json!({ "restarted": false }));
        }
        dev.restart_app_mode().map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "restarted": true }))
    })())
}

/// What a sync would change: `{"preview": …}` or `{"error": "…"}`. Takes
/// seconds (device and network): call it off the UI thread.
#[unsafe(no_mangle)]
pub extern "C" fn nimrag_preview_json() -> *mut c_char {
    answer(sync::preview_here().map(|p| serde_json::json!({ "preview": p })))
}

/// Progress of a sync: called with each step's text and the caller's `context`.
pub type StepCallback = extern "C" fn(step: *const c_char, context: *mut std::ffi::c_void);

/// Runs a sync as inReach Sync does, calling `step` (on this thread) before
/// each step. `{"applied": …}` or `{"error": "…", "step": "…"}`. Takes up to a
/// minute: call it off the UI thread.
#[unsafe(no_mangle)]
pub extern "C" fn nimrag_sync_apply(step: Option<StepCallback>, context: *mut std::ffi::c_void) -> *mut c_char {
    let mut last = String::new();
    let result = sync::apply_here(&mut |s: &str| {
        last = s.to_string();
        if let (Some(cb), Ok(text)) = (step, CString::new(s)) {
            cb(text.as_ptr(), context);
        }
    });
    to_c(match result {
        Ok(applied) => serde_json::json!({ "applied": applied }).to_string(),
        Err(e) => serde_json::json!({ "error": e, "step": last }).to_string(),
    })
}

/// Software updates for the inReach on its drive: `{"check": …}` or `{"error": "…"}`.
/// Asks Garmin: call it off the UI thread.
#[unsafe(no_mangle)]
pub extern "C" fn nimrag_update_check_json(reinstall: c_int) -> *mut c_char {
    answer(
        crate::firmware::check(&crate::updates::Client::default(), reinstall != 0)
            .map(|c| serde_json::json!({ "check": c })),
    )
}

/// Progress of a download: bytes so far and in all, and the caller's `context`.
pub type ProgressCallback = extern "C" fn(done: u64, total: u64, context: *mut std::ffi::c_void);

/// Downloads, checks and puts the updates on the inReach's drive, then ejects
/// it: `{"installed": …}` or `{"error": "…", "step": "…"}`. Takes minutes:
/// call it off the UI thread; `step` and `progress` are called on it.
#[unsafe(no_mangle)]
pub extern "C" fn nimrag_update_install(
    reinstall: c_int,
    step: Option<StepCallback>,
    progress: Option<ProgressCallback>,
    context: *mut std::ffi::c_void,
) -> *mut c_char {
    let mut last = String::new();
    let result = crate::firmware::install(
        &crate::updates::Client::default(),
        reinstall != 0,
        &mut |s: &str| {
            last = s.to_string();
            if let (Some(cb), Ok(text)) = (step, CString::new(s)) {
                cb(text.as_ptr(), context);
            }
        },
        &mut |done, total| {
            if let Some(cb) = progress {
                cb(done, total, context);
            }
        },
    );
    to_c(match result {
        Ok(done) => serde_json::json!({ "installed": done }).to_string(),
        Err(e) => serde_json::json!({ "error": e, "step": last }).to_string(),
    })
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
