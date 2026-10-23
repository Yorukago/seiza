/* SEIZA: command dispatch. See cmd.h. */
#include <ctype.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "cmd.h"
#include "constellation.h"

#define ARGS_MAX 4

/* Split `s` in place on whitespace. Returns the number of words (capped). */
static int split_words(char *s, char *words[], int max) {
	int n = 0;
	while (*s && n < max) {
		while (isspace((unsigned char)*s)) {
			*s++ = '\0';
		}
		if (!*s) {
			break;
		}
		words[n++] = s;
		while (*s && !isspace((unsigned char)*s)) {
			s++;
		}
	}
	if (*s) { /* more words than max */
		*s = '\0';
		return max + 1;
	}
	return n;
}

static int parse_int(const char *s, int *out) {
	char *end;
	long v = strtol(s, &end, 10);
	if (end == s || *end != '\0' || v < -100000 || v > 100000) {
		return -1;
	}
	*out = (int)v;
	return 0;
}

/* A coordinate: a finite number, nothing after it. */
static int parse_num(const char *s, double *out) {
	char *end;
	double v = strtod(s, &end);
	if (end == s || *end != '\0' || !isfinite(v) || fabs(v) > 1e7) {
		return -1;
	}
	*out = v;
	return 0;
}

static int reply(char *out, size_t outn, int rc, const char *err) {
	snprintf(out, outn, "%s%s", rc == 0 ? "ok" : "error: ", rc == 0 ? "" : err);
	return rc;
}

int cmd_run(const struct cmd_ops *ops, void *ud, const char *line, char *out, size_t outn) {
	char buf[1100]; /* a bit over the socket's 1 KiB line limit */
	if (strlen(line) >= sizeof(buf)) {
		return reply(out, outn, -1, "command too long");
	}
	snprintf(buf, sizeof(buf), "%s", line);
	/* trim */
	char *start = buf;
	while (isspace((unsigned char)*start)) {
		start++;
	}
	size_t len = strlen(start);
	while (len > 0 && isspace((unsigned char)start[len - 1])) {
		start[--len] = '\0';
	}
	if (len == 0) {
		return reply(out, outn, -1, "empty command");
	}

	/* Not a seiza command: it is a shell command line. */
	if (strncmp(start, "seiza", 5) != 0 || (start[5] != '\0' && !isspace((unsigned char)start[5]))) {
		ops->spawn(ud, start);
		return reply(out, outn, 0, "");
	}

	char *w[ARGS_MAX + 2];
	int n = split_words(start, w, ARGS_MAX + 1); /* w[0] is "seiza" */
	if (n < 2) {
		return reply(out, outn, -1, "missing command");
	}
	const char *verb = w[1];
	int args = n - 2;
	#define IS(v) (strcmp(verb, v) == 0)
	#define NEED(k) do { if (args != (k)) return reply(out, outn, -1, "wrong number of arguments"); } while (0)

	if (IS("overview")) { NEED(0); ops->overview(ud); return reply(out, outn, 0, ""); }
	if (IS("quit")) { NEED(0); ops->quit(ud); return reply(out, outn, 0, ""); }
	if (IS("zoom-reset")) { NEED(0); ops->zoom_reset(ud); return reply(out, outn, 0, ""); }
	if (IS("cycle-window")) { NEED(0); ops->cycle_window(ud); return reply(out, outn, 0, ""); }
	if (IS("new-star")) { NEED(0); return reply(out, outn, ops->new_star(ud), "could not create star"); }
	if (IS("delete-star")) {
		int which = 0; /* no number: the current star */
		if (args > 1 || (args == 1 && (parse_int(w[2], &which) != 0 || which < 1))) {
			return reply(out, outn, -1, "delete-star takes an optional star number");
		}
		return reply(out, outn, ops->delete_star(ud, which), "cannot delete (no such star, it has windows, or it is the last one)");
	}
	if (IS("add-star") || IS("move-star")) {
		int which = 0, base = IS("move-star") ? 1 : 0;
		double x, y;
		if (args != 2 + base || (base && (parse_int(w[2], &which) != 0 || which < 1)) ||
				parse_num(w[2 + base], &x) != 0 || parse_num(w[3 + base], &y) != 0) {
			return reply(out, outn, -1, base ? "move-star needs a star number and x y" : "add-star needs x y");
		}
		return reply(out, outn, base ? ops->move_star(ud, which, x, y) : ops->add_star_at(ud, x, y), "no room there, or no such star");
	}
	if (IS("debug-windows")) { NEED(0); ops->debug_windows(ud); return reply(out, outn, 0, ""); }
	if (IS("debug-pointer")) {
		double x, y;
		if (args < 2 || args > 3 || parse_num(w[2], &x) != 0 || parse_num(w[3], &y) != 0 || (args == 3 && strcmp(w[4], "click") != 0 && strcmp(w[4], "zoom") != 0)) {
			return reply(out, outn, -1, "debug-pointer needs x y [click|zoom]");
		}
		return reply(out, outn, ops->debug_pointer(ud, x, y, args == 3 ? (strcmp(w[4], "click") == 0 ? 1 : 2) : 0), "debug input is off (set SEIZA_DEBUG_INPUT)");
	}
	if (IS("link")) {
		int a, b;
		if (args != 2 || parse_int(w[2], &a) != 0 || parse_int(w[3], &b) != 0) {
			return reply(out, outn, -1, "link needs two star numbers");
		}
		return reply(out, outn, ops->toggle_link(ud, a, b), "no such stars");
	}
	if (IS("rename-star")) {
		int which;
		if (args < 2 || parse_int(w[2], &which) != 0 || which < 1) {
			return reply(out, outn, -1, "rename-star needs a star number and a name");
		}
		char name[128] = ""; /* the rest of the words, one space apart */
		for (int i = 3; i < n; i++) {
			snprintf(name + strlen(name), sizeof(name) - strlen(name), "%s%s", i > 3 ? " " : "", w[i]);
		}
		return reply(out, outn, ops->rename_star(ud, which, name), "no such star, or a name that will not do");
	}
	if (IS("relaunch")) { NEED(0); return reply(out, outn, ops->relaunch(ud), "relaunch failed"); }
	if (IS("reload-config")) { NEED(0); return reply(out, outn, ops->reload_config(ud), "reload failed"); }
	if (IS("save") || IS("load")) {
		NEED(1);
		if (!constellation_name_ok(w[2])) {
			return reply(out, outn, -1, "bad name (use letters, digits, _ and -)");
		}
		int rc = IS("save") ? ops->save(ud, w[2]) : ops->load(ud, w[2]);
		return reply(out, outn, rc, IS("save") ? "save failed" : "no such constellation");
	}
	if (IS("pan")) {
		NEED(2);
		int dx, dy;
		if (parse_int(w[2], &dx) != 0 || parse_int(w[3], &dy) != 0) {
			return reply(out, outn, -1, "pan needs two integers");
		}
		ops->pan(ud, dx, dy);
		return reply(out, outn, 0, "");
	}
	if (IS("goto")) {
		NEED(1);
		int n;
		if (parse_int(w[2], &n) != 0) {
			return reply(out, outn, -1, "goto needs a star number");
		}
		return reply(out, outn, ops->goto_star(ud, n), "no such star");
	}
	if (IS("help")) { NEED(0); ops->help(ud); return reply(out, outn, 0, ""); }
	if (IS("figure")) { NEED(1); return reply(out, outn, ops->set_figure(ud, w[2]), "no such figure"); }
	if (IS("maximize")) { NEED(0); ops->toggle_maximize(ud); return reply(out, outn, 0, ""); }
	if (IS("close-window")) { NEED(0); ops->close_window(ud); return reply(out, outn, 0, ""); }
	if (IS("move-window") || IS("resize-window") || IS("focus")) {
		NEED(2);
		int a, b;
		if (parse_int(w[2], &a) != 0 || parse_int(w[3], &b) != 0) {
			return reply(out, outn, -1, "needs two integers");
		}
		if (IS("move-window")) ops->move_window(ud, a, b);
		else if (IS("resize-window")) ops->resize_window(ud, a, b);
		else ops->focus_dir(ud, a, b);
		return reply(out, outn, 0, "");
	}
	return reply(out, outn, -1, "unknown command");
	#undef IS
	#undef NEED
}
