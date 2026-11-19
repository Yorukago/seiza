/* SEIZA: how the camera moves. `~/.config/seiza/motion`, lines of `key=value`:
 *     pan_secs = 0.35          how long a pan or fly takes
 *     easing   = drift         drift (fast, then settles), glide (slow at both ends), float (even slower start),
 *                              or the name of a Hyprland curve (motion_curve): expressiveFastSpatial bounces
 *     reduce   = 1             no gliding: the camera jumps */
#ifndef SEIZA_MOTION_H
#define SEIZA_MOTION_H

enum { EASE_DRIFT, EASE_GLIDE, EASE_FLOAT };

struct motion {
	double pan_secs;
	int easing;
	const double *curve; /* a named bezier from motion_curve, used instead of `easing` when set */
	int reduce;
};

void motion_defaults(struct motion *m);

/* Apply the lines of `text` on top of `m`; bad lines are skipped. */
void motion_parse(const char *text, struct motion *m);

/* A CSS cubic-bezier c = {x1, y1, x2, y2}: time p in 0..1 -> eased value (above 1 = overshoot, the bounce). */
double motion_bezier(const double c[4], double p);

/* The curves of the user's Hyprland rice by name ("emphasizedDecel", "expressiveFastSpatial", ...); NULL if unknown. */
const double *motion_curve(const char *name);

/* How long a pan takes, and the eased progress for p in 0..1. */
double motion_duration(const struct motion *m);
double motion_ease(const struct motion *m, double p);

#endif
