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
	int  (*delete_star)(void *ud, int n); /* star n (1-based; 0 = the current one); -1 if it has windows, is the last one, or is not there */
	int  (*save)(void *ud, const char *name);
	int  (*load)(void *ud, const char *name);
	int  (*new_constellation)(void *ud, const char *name); /* a fresh one with a single star, opened; -1 if the name is taken */
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
	int  (*set_figure)(void *ud, const char *name);  /* lay the stars out on a figure; "none" lets go; -1 if unknown */
	int  (*rename_star)(void *ud, int n, const char *name); /* star n, 1-based; -1 if there is no such star or the name will not do */
	int  (*add_star_at)(void *ud, double x, double y); /* a star centered there; -1 if there is no room */
	int  (*move_star)(void *ud, int n, double x, double y); /* star n (1-based) to there; -1 if no such star or no room */
	void (*debug_windows)(void *ud);                 /* log every window Seiza knows of: where, how big, what kind */
	int  (*debug_pointer)(void *ud, double x, double y, int act); /* tests only: move the pointer, then 1 = click, 2 = zoom out a step; -1 unless SEIZA_DEBUG_INPUT is set */
	int  (*toggle_link)(void *ud, int a, int b);     /* a thread between stars a and b (1-based), or none if there was one; -1 if no such stars */
	void (*switcher)(void *ud);                      /* write the window list (most recent first) for seiza-switcher and start it */
	int  (*focus_window)(void *ud, int n);           /* window n of that list (1-based): fly to its star and focus it; -1 if no such window */
	void (*toggle_tile)(void *ud);                   /* tile or untile the star the camera is on */
	int  (*night)(void *ud, const char *mode);       /* night light: "on", "off" or "auto" (the schedule); -1 if it is none of those */
	int  (*keymap)(void *ud, const char *layout);    /* every keyboard to this xkb layout ("pt", "br", "us"...); -1 if xkb does not know it */
};

/* Run one command line. `seiza <verb> [args]` goes to the ops table; any other
 * line is a shell command and goes to ops->spawn. A one-line reply ("ok" or
 * "error: ...") is written to `out`. Returns 0 on success, -1 on error. */
int cmd_run(const struct cmd_ops *ops, void *ud, const char *line, char *out, size_t outn);

#endif
