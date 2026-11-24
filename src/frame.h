/* SEIZA: how a window's frame looks. `~/.config/seiza/frame`, lines of `key=value`:
 *     radius = 14        rounded corners, 0 (square) to 32
 *     screen = 22        the screen's own rounded corners, 0 (off) to 48
 *     border = 2         the rim around a window, 0 to 8
 *     glow   = soft      the bloom around the focused window: off, soft or bright */
#ifndef SEIZA_FRAME_H
#define SEIZA_FRAME_H

enum { GLOW_OFF, GLOW_SOFT, GLOW_BRIGHT };

struct frame {
	int radius;
	int screen;
	int border;
	int glow;
};

void frame_defaults(struct frame *f);

/* Apply the lines of `text` on top of `f`; bad lines and out-of-range numbers are skipped. */
void frame_parse(const char *text, struct frame *f);

#endif
