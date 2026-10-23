#include "motion.h"
#include <math.h>
#include <stdlib.h>
#include <string.h>

void motion_defaults(struct motion *m) {
	m->pan_secs = 0.35;
	m->easing = EASE_DRIFT;
	m->reduce = 0;
}

void motion_parse(const char *text, struct motion *m) {
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
				if (!strcmp(key, "pan_secs")) {
					char *stop;
					double v = strtod(val, &stop);
					if (stop != val && *stop == '\0') m->pan_secs = fmin(3.0, fmax(0.05, v));
				} else if (!strcmp(key, "easing")) {
					if (!strcmp(val, "drift")) m->easing = EASE_DRIFT;
					else if (!strcmp(val, "glide")) m->easing = EASE_GLIDE;
					else if (!strcmp(val, "float")) m->easing = EASE_FLOAT;
				} else if (!strcmp(key, "reduce")) {
					m->reduce = !strcmp(val, "1");
				}
			}
		}
		line = end ? end + 1 : line + len;
	}
}

double motion_duration(const struct motion *m) {
	return m->reduce ? 0.01 : m->pan_secs;
}

double motion_ease(const struct motion *m, double p) {
	switch (m->easing) {
	case EASE_GLIDE: return p * p * (3.0 - 2.0 * p);
	case EASE_FLOAT: return 0.5 - 0.5 * cos(M_PI * p);
	default: return 1.0 - pow(1.0 - p, 3.0);
	}
}
