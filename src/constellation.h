/* SEIZA: the saved shape of a constellation (stars, names, positions, remembered
 * apps) and its plain-text file format. No wlroots in here, so `make test` can
 * cover it.
 *
 * File format, one thing per line, '#' starts a comment line:
 *     constellation <name>
 *     star <x> <y> <star name, rest of the line>
 *     app <command line, rest of the line>       (belongs to the star above)
 *     end                                        (a file without it was cut short)
 */
#ifndef SEIZA_CONSTELLATION_H
#define SEIZA_CONSTELLATION_H

#include <stddef.h>

#define CSTAR_APPS_MAX 8
#define CONST_STARS_MAX 64

struct cstar {
	char name[32];
	double x, y; /* center of the star's territory, world coordinates */
	char apps[CSTAR_APPS_MAX][256];
	int napps;
};

struct constellation {
	char name[32];
	struct cstar stars[CONST_STARS_MAX];
	int nstars;
};

/* 0 ok, -1 not a valid constellation file. */
int constellation_parse(const char *text, struct constellation *out);
/* Bytes written (without the NUL), or -1 if `n` is too small. */
int constellation_format(const struct constellation *c, char *buf, size_t n);
/* Atomic (temp file + rename) write of <dir>/<name>.const; creates dir. 0 ok. */
int constellation_save(const struct constellation *c, const char *dir);
/* Reads <dir>/<name>.const. A file that does not parse is renamed to
 * <name>.const.bad and -1 is returned. A missing file also returns -1. */
int constellation_load(const char *dir, const char *name, struct constellation *out);
/* Names are [A-Za-z0-9_-]{1,31}, not starting with '-': safe as file names. */
int constellation_name_ok(const char *name);
/* "default" with a single star "star 0" at 960,540. */
void constellation_default(struct constellation *out);
/* `last` file: which constellation to open on the next start. */
int constellation_set_last(const char *dir, const char *name);
int constellation_get_last(const char *dir, char *name, size_t n);
/* Remember an app on a star: de-duplicated, capped. 1 if added, 0 if not. */
int constellation_add_app(struct cstar *s, const char *cmdline);
/* $XDG_CONFIG_HOME/seiza or ~/.config/seiza, into buf. */
void constellation_config_dir(char *buf, size_t n);

#endif
