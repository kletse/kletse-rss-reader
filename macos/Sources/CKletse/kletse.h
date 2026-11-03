// The Rust core's C interface (src/ffi.rs). Commands and replies are JSON.
#pragma once

typedef struct KRCore KRCore;

// Called from a background thread when answers are waiting: send {"cmd":"pump"} on the main thread.
typedef void (*KRNotify)(void *ctx);

KRCore *kr_new(KRNotify notify, void *ctx);
// One command; the reply is freed with kr_free_string. Main thread only.
char *kr_send(KRCore *core, const char *command);
void kr_free_string(char *reply);
