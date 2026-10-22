/* SEIZA: the control socket ($XDG_RUNTIME_DIR/seiza.sock) and the client side of
 * the same binary. Protocol: the client sends one line ("seiza <verb> ...\n"),
 * the compositor answers one line ("ok" or "error: ...") and closes. */
#ifndef SEIZA_IPC_H
#define SEIZA_IPC_H

#include <wayland-server-core.h>
#include "cmd.h"

/* Start listening. 0 ok; -1 if another compositor owns the socket, or on error
 * (the compositor then simply runs without a socket). */
int ipc_serve(struct wl_event_loop *loop, const struct cmd_ops *ops, void *ud);
/* Remove the socket file (call at exit). */
void ipc_close(void);
/* Client mode: `seiza <verb> ...` from a shell. Returns the process exit code. */
int ipc_client(int argc, char **argv);

#endif
