/* SEIZA: process info from /proc. See procinfo.h. */
#include <stdio.h>
#include <string.h>
#include "procinfo.h"

/* Append one argument, single-quoted if it has anything a shell would treat
 * specially. Returns the new length or -1 if it does not fit. */
static int append_arg(char *buf, size_t n, size_t used, const char *arg) {
	int plain = *arg != '\0';
	for (const char *c = arg; *c; c++) {
		if (!strchr("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_@%+=:,./-", *c)) {
			plain = 0;
		}
	}
	size_t need = used + (used ? 1 : 0) + strlen(arg) * 4 + 3;
	if (need >= n) {
		return -1;
	}
	char *o = buf + used;
	if (used) {
		*o++ = ' ';
	}
	if (plain) {
		o += sprintf(o, "%s", arg);
	} else {
		*o++ = '\'';
		for (const char *c = arg; *c; c++) {
			if (*c == '\'') {
				o += sprintf(o, "'\\''");
			} else {
				*o++ = *c;
			}
		}
		*o++ = '\'';
	}
	*o = '\0';
	return (int)(o - buf);
}

int procinfo_cmdline(pid_t pid, char *buf, size_t n) {
	if (pid <= 0 || n < 2) {
		return -1;
	}
	char path[64], raw[4096];
	snprintf(path, sizeof(path), "/proc/%d/cmdline", (int)pid);
	FILE *f = fopen(path, "r");
	if (!f) {
		return -1;
	}
	size_t len = fread(raw, 1, sizeof(raw) - 1, f);
	fclose(f);
	if (len == 0) { /* kernel thread or already gone */
		return -1;
	}
	raw[len] = '\0';
	buf[0] = '\0';
	size_t used = 0;
	for (size_t i = 0; i < len;) { /* arguments are NUL-separated */
		int r = append_arg(buf, n, used, raw + i);
		if (r < 0) {
			return -1;
		}
		used = r;
		i += strlen(raw + i) + 1;
	}
	return 0;
}
