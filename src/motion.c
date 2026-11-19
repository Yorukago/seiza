#include "motion.h"
#include <math.h>
#include <stdlib.h>
#include <string.h>

static const struct { const char *name; double c[4]; } CURVES[] = {
	{"calm",                     {0.05, 0.70, 0.10, 1.00}}, /* the two names the settings app shows */
	{"bounce",                   {0.42, 1.67, 0.21, 0.90}},
	{"emphasizedDecel",          {0.05, 0.70, 0.10, 1.00}},
	{"expressiveFastSpatial",    {0.42, 1.67, 0.21, 0.90}},
	{"expressiveSlowSpatial",    {0.39, 1.29, 0.35, 0.98}},
	{"expressiveDefaultSpatial", {0.38, 1.21, 0.22, 1.00}},
	{"emphasizedAccel",          {0.30, 0.00, 0.80, 0.15}},
	{"standardDecel",            {0.00, 0.00, 0.00, 1.00}},
	{"menu_decel",               {0.10, 1.00, 0.00, 1.00}},
	{"menu_accel",               {0.52, 0.03, 0.72, 0.08}},
	{"stall",                    {1.00, -0.10, 0.70, 0.85}},
};

const double *motion_curve(const char *name) {
	for (size_t i = 0; i < sizeof(CURVES) / sizeof(CURVES[0]); i++) {
		if (!strcmp(CURVES[i].name, name)) {
			return CURVES[i].c;
		}
	}
	return NULL;
}

/* Bisection on x(t); 20 steps is plenty for a screen. */
double motion_bezier(const double c[4], double p) {
	double x1 = c[0], y1 = c[1], x2 = c[2], y2 = c[3];
	double lo = 0, hi = 1, t = p, u;
	for (int i = 0; i < 20; i++) {
		t = (lo + hi) / 2;
		u = 1 - t;
		if (3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t < p) {
			lo = t;
		} else {
			hi = t;
		}
	}
	u = 1 - t;
	return 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t;
}

void motion_defaults(struct motion *m) {
	m->pan_secs = 0.35;
	m->easing = EASE_DRIFT;
	m->curve = NULL;
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
					const double *c = motion_curve(val);
					if (c) m->curve = c;
					else if (!strcmp(val, "drift")) { m->easing = EASE_DRIFT; m->curve = NULL; }
					else if (!strcmp(val, "glide")) { m->easing = EASE_GLIDE; m->curve = NULL; }
					else if (!strcmp(val, "float")) { m->easing = EASE_FLOAT; m->curve = NULL; }
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
	if (m->curve) return motion_bezier(m->curve, p);
	switch (m->easing) {
	case EASE_GLIDE: return p * p * (3.0 - 2.0 * p);
	case EASE_FLOAT: return 0.5 - 0.5 * cos(M_PI * p);
	default: return 1.0 - pow(1.0 - p, 3.0);
	}
}
