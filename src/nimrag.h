// The C interface of the Rust core (core/src/ffi.rs). Every returned string is
// UTF-8 JSON owned by the core; give it back with nimrag_string_free.
#pragma once

#ifdef __cplusplus
extern "C" {
#endif

// The connected inReach devices as a JSON array. With ask non-zero each one is
// asked for its mode and version, which can take seconds.
char *nimrag_devices_json(int ask);

// Restarts the inReach at node as a USB drive: {"switched": bool} or {"error": "…"}.
char *nimrag_switch_to_drive(const char *node);

// The signed-in account: {"email": "…"} or {"email": null}.
char *nimrag_account_json(void);

// Signs in (the password goes to the keyring): {"email": "…"} or {"error": "…"}.
char *nimrag_sign_in(const char *email, const char *password);

// Signs out: {"email": "…" or null} or {"error": "…"}.
char *nimrag_sign_out(void);

// Restarts the inReach at node in app mode (it switches on): {"restarted": bool} or {"error": "…"}.
char *nimrag_switch_on(const char *node);

// What a sync would change: {"preview": …} or {"error": "…"}. Takes seconds.
char *nimrag_preview_json(void);

// Runs a sync, calling step(text, context) on the calling thread before each
// step: {"applied": …} or {"error": "…", "step": "…"}. Takes up to a minute.
typedef void (*nimrag_step_callback)(const char *step, void *context);
char *nimrag_sync_apply(nimrag_step_callback step, void *context);

// Software updates for the inReach on its USB drive: {"check": …} or {"error": "…"}.
char *nimrag_update_check_json(int reinstall);

// Downloads, checks and copies the updates to the drive, then ejects it,
// calling step and progress on the calling thread: {"installed": …} or
// {"error": "…", "step": "…"}. Takes minutes.
typedef void (*nimrag_progress_callback)(unsigned long long done, unsigned long long total, void *context);
char *nimrag_update_install(int reinstall, nimrag_step_callback step, nimrag_progress_callback progress, void *context);

// The command line; returns the exit code.
int nimrag_cli_main(int argc, const char *const *argv);

void nimrag_string_free(char *text);

#ifdef __cplusplus
}
#endif
