#include "night.h"
#include <math.h>

#define RAMP_MIN 30.0

double night_factor(int minute, int from, int to) {
	int len = (to - from + 1440) % 1440;
	if (len == 0) {
		return 0;
	}
	int in = (minute - from + 1440) % 1440;
	if (in < len) {
		return fmin(1.0, in / RAMP_MIN);
	}
	int out = (minute - to + 1440) % 1440; /* minutes since the window closed */
	return out < RAMP_MIN ? 1.0 - out / RAMP_MIN : 0.0;
}

/* Tanner Helland's fit of the black-body colour, good for 1000..40000 K. */
void night_rgb(double kelvin, double rgb[3]) {
	if (kelvin >= 6500) {
		rgb[0] = rgb[1] = rgb[2] = 1.0;
		return;
	}
	double t = fmax(kelvin, 1000) / 100.0, r = 255, g, b;
	g = 99.4708025861 * log(t) - 161.1195681661;
	b = t >= 66 ? 255 : t <= 19 ? 0 : 138.5177312231 * log(t - 10) - 305.0447927307;
	rgb[0] = r / 255;
	rgb[1] = fmin(1.0, fmax(0.0, g / 255));
	rgb[2] = fmin(1.0, fmax(0.0, b / 255));
}
