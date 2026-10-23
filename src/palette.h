/* SEIZA: the two palette colors the compositor needs (the satellites read the
 * rest). Same file as the satellites: `role=#rrggbb` lines in
 * ~/.config/seiza/palette; unknown roles are ignored. */
#ifndef SEIZA_PALETTE_H
#define SEIZA_PALETTE_H

struct palette {
	float sky[3];  /* background */
	float star[3]; /* star glow, window frame: gold unless the file says otherwise */
};

void palette_defaults(struct palette *p);

/* Apply the lines of `text` on top of `p`; bad lines are skipped. */
void palette_parse(const char *text, struct palette *p);

#endif
