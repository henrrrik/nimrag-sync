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

// The command line; returns the exit code.
int nimrag_cli_main(int argc, const char *const *argv);

void nimrag_string_free(char *text);

#ifdef __cplusplus
}
#endif
