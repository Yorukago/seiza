/* SEIZA: how the camera moves. `~/.config/seiza/motion`, lines of `key=value`:
 *     pan_secs = 0.35          how long a pan or fly takes
 *     easing   = drift         drift (fast, then settles), glide (slow at both ends), float (even slower start)
 *     reduce   = 1             no gliding: the camera jumps */
#ifndef SEIZA_MOTION_H
#define SEIZA_MOTION_H

enum { EASE_DRIFT, EASE_GLIDE, EASE_FLOAT };

struct motion {
	double pan_secs;
	int easing;
	int reduce;
};

void motion_defaults(struct motion *m);

/* Apply the lines of `text` on top of `m`; bad lines are skipped. */
void motion_parse(const char *text, struct motion *m);

/* How long a pan takes, and the eased progress for p in 0..1. */
double motion_duration(const struct motion *m);
double motion_ease(const struct motion *m, double p);

#endif
