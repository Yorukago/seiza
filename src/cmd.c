/* SEIZA: command dispatch. See cmd.h. */
#include <ctype.h>
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

static int reply(char *out, size_t outn, int rc, const char *err) {
	snprintf(out, outn, "%s%s", rc == 0 ? "ok" : "error: ", rc == 0 ? "" : err);
	return rc;
}

int cmd_run(const struct cmd_ops *ops, void *ud, const char *line, char *out, size_t outn) {
	char buf[512];
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
		NEED(0);
		return reply(out, outn, ops->delete_star(ud), "cannot delete (star has windows, or it is the last one)");
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
	return reply(out, outn, -1, "unknown command");
	#undef IS
	#undef NEED
}
