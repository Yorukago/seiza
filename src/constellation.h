/* SEIZA: the saved shape of a constellation (stars, names, positions, remembered
 * apps) and its plain-text file format. No wlroots in here, so `make test` can
 * cover it.
 *
 * File format, one thing per line, '#' starts a comment line:
 *     constellation <name>
 *     star <x> <y> <star name, rest of the line>
 *     app <command line, rest of the line>       (belongs to the star above)
 *     tile 1|-1                                  (the star above is always / never tiled; left out = follow the config)
 *     link <a> <b>                               (a thread between stars a and b, counting from 0 in file order)
 *     point <n>, figure <name>                   (older files: read, and turned into links by the compositor; never written)
 *     end                                        (a file without it was cut short)
 */
#ifndef SEIZA_CONSTELLATION_H
#define SEIZA_CONSTELLATION_H

#include <stddef.h>

#define CSTAR_APPS_MAX 8
#define CONST_STARS_MAX 64
#define CONST_LINKS_MAX 128

struct cstar {
	char name[32];
	double x, y; /* center of the star's territory, world coordinates */
	char apps[CSTAR_APPS_MAX][256];
	int napps;
	int tiled; /* 0 follows the layout config, 1 tiled, -1 floating */
	int fig_point; /* legacy: 1-based point of the old figure this star held; 0 = none */
};

struct constellation {
	char name[32];
	char figure[32]; /* legacy: the figure older files recorded; "" = none */
	int links[CONST_LINKS_MAX][2]; /* threads, as indexes into stars */
	int nlinks;
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
