/* SEIZA: the control socket ($XDG_RUNTIME_DIR/seiza.sock) and the client side of
 * the same binary. Protocol: the client sends one line ("seiza <verb> ...\n"),
 * the compositor answers one line ("ok" or "error: ...") and closes.
 *
 * Exception: "seiza subscribe" keeps the connection open. The compositor sends
 * the current state as one JSON object per line, now and again every time it
 * changes (see publish_state in seiza.c for the fields). Satellites read that
 * and send ordinary commands on a second connection. */
#ifndef SEIZA_IPC_H
#define SEIZA_IPC_H

#include <wayland-server-core.h>
#include "cmd.h"

/* Start listening. 0 ok; -1 if another compositor owns the socket, or on error
 * (the compositor then simply runs without a socket). */
int ipc_serve(struct wl_event_loop *loop, const struct cmd_ops *ops, void *ud);
/* Send `json` (one line, no newline) to every subscriber, unless it equals the
 * last one sent. Remembered for subscribers who connect later. */
void ipc_publish(const char *json);
/* Remove the socket file (call at exit). */
void ipc_close(void);
/* Client mode: `seiza <verb> ...` from a shell. Returns the process exit code. */
int ipc_client(int argc, char **argv);

#endif
