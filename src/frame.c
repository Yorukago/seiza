#include "frame.h"
#include <stdlib.h>
#include <string.h>

void frame_defaults(struct frame *f) {
	f->radius = 14;
	f->screen = 22;
	f->border = 2;
	f->glow = GLOW_SOFT;
}

static int number(const char *s, int lo, int hi, int *out) {
	char *end;
	long v = strtol(s, &end, 10);
	if (end == s || *end != '\0' || v < lo || v > hi) {
		return -1;
	}
	*out = (int)v;
	return 0;
}

void frame_parse(const char *text, struct frame *f) {
	for (const char *line = text; *line;) {
		const char *end = strchr(line, '\n');
		size_t len = end ? (size_t)(end - line) : strlen(line);
		char buf[64];
		if (len < sizeof(buf)) {
			memcpy(buf, line, len);
			buf[len] = '\0';
			char *eq = strchr(buf, '=');
			if (eq) {
				*eq = '\0';
				char *key = buf, *val = eq + 1;
				while (*key == ' ') key++;
				for (char *e = key + strlen(key); e > key && e[-1] == ' '; ) *--e = '\0';
				while (*val == ' ') val++;
				for (char *e = val + strlen(val); e > val && e[-1] == ' '; ) *--e = '\0';
				if (!strcmp(key, "radius")) {
					number(val, 0, 32, &f->radius);
				} else if (!strcmp(key, "screen")) {
					number(val, 0, 48, &f->screen);
				} else if (!strcmp(key, "border")) {
					number(val, 0, 8, &f->border);
				} else if (!strcmp(key, "glow")) {
					if (!strcmp(val, "off")) f->glow = GLOW_OFF;
					else if (!strcmp(val, "soft")) f->glow = GLOW_SOFT;
					else if (!strcmp(val, "bright")) f->glow = GLOW_BRIGHT;
				}
			}
		}
		line = end ? end + 1 : line + len;
	}
}
