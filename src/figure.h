/* SEIZA: constellation figures. A figure is a template for where a constellation's
 * stars start (points, 0..1) and which of them the threads join. Stars stay free to
 * move afterwards; each star only remembers which point it holds.
 *
 * File `~/.config/seiza/figures/<name>.fig`, one thing per line, '#' comments:
 *     point 0.50 0.10          (in order: the first star takes the first point)
 *     link 0 1                 (a thread between points 0 and 1)
 * A few figures are built in; a file of the same name wins. No wlroots in here. */
#ifndef SEIZA_FIGURE_H
#define SEIZA_FIGURE_H

#define FIG_POINTS_MAX 24
#define FIG_LINKS_MAX 40

struct figure {
	char name[32];
	int n;
	double u[FIG_POINTS_MAX], v[FIG_POINTS_MAX];
	int nlinks;
	int links[FIG_LINKS_MAX][2];
};

/* 0 ok, -1 not a figure (no points, a link to a missing point, bad line). */
int figure_parse(const char *text, struct figure *out);

/* The figure called `name`: <dir>/figures/<name>.fig, else a built-in. 0 ok, -1 none. */
int figure_find(const char *dir, const char *name, struct figure *out);

/* Built-in figures, by index; NULL past the end. */
const char *figure_preset_name(int i);

/* The smallest scale (world units per 0..1) at which no two points are closer than
 * one star territory apart, `dx` by `dy` (the spacing between neighbouring stars). */
double figure_scale(const struct figure *f, double dx, double dy);

/* World position of point `i`, for that scale; the whole figure sits in the
 * positive quarter, its top-left point at (x0, y0). */
void figure_point(const struct figure *f, int i, double scale, double x0, double y0, double *x, double *y);

#endif
