/* SEIZA: the one place every action goes through. Key bindings, the socket and
 * the `seiza <verb>` command line all end up in cmd_run(). It knows nothing
 * about wlroots: the compositor hands it a table of operations (cmd_ops), and
 * the tests hand it a fake one. */
#ifndef SEIZA_CMD_H
#define SEIZA_CMD_H

#include <stddef.h>

struct cmd_ops {
	void (*overview)(void *ud);
	int  (*new_star)(void *ud);
	int  (*delete_star)(void *ud);   /* -1 if the star has windows, or is the last one */
	int  (*save)(void *ud, const char *name);
	int  (*load)(void *ud, const char *name);
	int  (*relaunch)(void *ud);
	int  (*reload_config)(void *ud);
	void (*quit)(void *ud);
	void (*pan)(void *ud, int dx, int dy);
	void (*zoom_reset)(void *ud);
	void (*cycle_window)(void *ud);
	void (*spawn)(void *ud, const char *cmdline); /* run through the configured shell */
	void (*move_window)(void *ud, int dx, int dy);   /* focused window, world pixels */
	void (*resize_window)(void *ud, int dw, int dh); /* focused window, grows right/down */
	void (*close_window)(void *ud);
	int  (*goto_star)(void *ud, int n);              /* 1-based, in creation order; -1 if no such star */
	void (*help)(void *ud);                          /* show the key bindings in a window */
	void (*focus_dir)(void *ud, int dx, int dy);     /* nearest window that way (signs only) */
	void (*toggle_maximize)(void *ud);               /* focused window */
};

/* Run one command line. `seiza <verb> [args]` goes to the ops table; any other
 * line is a shell command and goes to ops->spawn. A one-line reply ("ok" or
 * "error: ...") is written to `out`. Returns 0 on success, -1 on error. */
int cmd_run(const struct cmd_ops *ops, void *ud, const char *line, char *out, size_t outn);

#endif
