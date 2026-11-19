/* SEIZA night light: the pure parts (the schedule and the colour), so `make test` covers them. */
#ifndef SEIZA_NIGHT_H
#define SEIZA_NIGHT_H

/* How warm the screen should be at `minute` of the day (0..1439) when it warms from `from` to `to` (minutes of
 * the day; the window may wrap past midnight). 0 = daylight, 1 = fully warm. It eases in over the 30 minutes
 * after `from` and back out over the 30 after `to`. from == to means never. */
double night_factor(int minute, int from, int to);

/* The red, green and blue multipliers (0..1) that make a white screen look like a `kelvin` light. 6500 and up leave it alone. */
void night_rgb(double kelvin, double rgb[3]);

#endif
