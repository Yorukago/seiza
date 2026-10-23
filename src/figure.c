#include "figure.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "constellation.h"

/* Built in, in the file syntax. Spread out evenly, so the figures do not come out huge. */
static const struct { const char *name, *text; } PRESETS[] = {
	{ "dipper",
	  "point 0.00 0.15\npoint 0.20 0.38\npoint 0.40 0.52\npoint 0.60 0.45\n"
	  "point 0.64 0.85\npoint 1.00 0.92\npoint 1.00 0.52\n"
	  "link 0 1\nlink 1 2\nlink 2 3\nlink 3 4\nlink 4 5\nlink 5 6\nlink 6 3\n" },
	{ "w",
	  "point 0.00 0.10\npoint 0.25 0.90\npoint 0.50 0.25\npoint 0.75 0.95\npoint 1.00 0.20\n"
	  "link 0 1\nlink 1 2\nlink 2 3\nlink 3 4\n" },
	{ "kite",
	  "point 0.50 0.00\npoint 0.90 0.38\npoint 0.50 0.76\npoint 0.10 0.38\npoint 0.50 1.30\n"
	  "link 0 1\nlink 1 2\nlink 2 3\nlink 3 0\nlink 2 4\n" },
	{ "crown",
	  "point 0.00 0.70\npoint 0.14 0.28\npoint 0.40 0.04\npoint 0.68 0.04\npoint 0.92 0.30\npoint 1.00 0.74\n"
	  "link 0 1\nlink 1 2\nlink 2 3\nlink 3 4\nlink 4 5\n" },
};
#define NPRESETS ((int)(sizeof(PRESETS) / sizeof(PRESETS[0])))

int figure_parse(const char *text, struct figure *out) {
	memset(out, 0, sizeof(*out));
	char *copy = strdup(text), *save = NULL;
	if (!copy) {
		return -1;
	}
	int ok = 1;
	for (char *line = strtok_r(copy, "\n", &save); line && ok; line = strtok_r(NULL, "\n", &save)) {
		if (line[0] == '#' || line[0] == '\0') {
			continue;
		} else if (strncmp(line, "point ", 6) == 0) {
			double u, v;
			ok = out->n < FIG_POINTS_MAX && sscanf(line + 6, "%lf %lf", &u, &v) == 2 && isfinite(u) && isfinite(v);
			if (ok) {
				out->u[out->n] = u;
				out->v[out->n++] = v;
			}
		} else if (strncmp(line, "link ", 5) == 0) {
			int a, b;
			ok = out->nlinks < FIG_LINKS_MAX && sscanf(line + 5, "%d %d", &a, &b) == 2 && a != b && a >= 0 && b >= 0;
			if (ok) {
				out->links[out->nlinks][0] = a;
				out->links[out->nlinks++][1] = b;
			}
		} else {
			ok = 0;
		}
	}
	free(copy);
	for (int i = 0; ok && i < out->nlinks; i++) {
		ok = out->links[i][0] < out->n && out->links[i][1] < out->n; /* a thread needs both its ends */
	}
	return (ok && out->n > 0) ? 0 : -1;
}

const char *figure_preset_name(int i) {
	return i >= 0 && i < NPRESETS ? PRESETS[i].name : NULL;
}

int figure_find(const char *dir, const char *name, struct figure *out) {
	if (!constellation_name_ok(name)) {
		return -1;
	}
	char path[512];
	snprintf(path, sizeof(path), "%s/figures/%s.fig", dir, name);
	FILE *f = fopen(path, "r");
	if (f) {
		char buf[8192];
		size_t n = fread(buf, 1, sizeof(buf) - 1, f);
		fclose(f);
		buf[n] = '\0';
		if (figure_parse(buf, out) == 0) {
			snprintf(out->name, sizeof(out->name), "%s", name);
			return 0;
		}
	}
	for (int i = 0; i < NPRESETS; i++) {
		if (strcmp(PRESETS[i].name, name) == 0 && figure_parse(PRESETS[i].text, out) == 0) {
			snprintf(out->name, sizeof(out->name), "%s", name);
			return 0;
		}
	}
	return -1;
}

double figure_scale(const struct figure *f, double dx, double dy) {
	double s = 1;
	for (int i = 0; i < f->n; i++) {
		for (int j = i + 1; j < f->n; j++) {
			double du = fabs(f->u[i] - f->u[j]), dv = fabs(f->v[i] - f->v[j]);
			if (du < 1e-9 && dv < 1e-9) {
				continue; /* two points in one place can never be told apart */
			}
			double need = fmin(du > 1e-9 ? dx / du : INFINITY, dv > 1e-9 ? dy / dv : INFINITY);
			s = fmax(s, need);
		}
	}
	return s;
}

void figure_point(const struct figure *f, int i, double scale, double x0, double y0, double *x, double *y) {
	double umin = f->u[0], vmin = f->v[0];
	for (int k = 1; k < f->n; k++) {
		umin = fmin(umin, f->u[k]);
		vmin = fmin(vmin, f->v[k]);
	}
	*x = x0 + (f->u[i] - umin) * scale;
	*y = y0 + (f->v[i] - vmin) * scale;
}
