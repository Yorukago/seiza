/* SEIZA: constellation file format and disk handling. See constellation.h. */
#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include "constellation.h"

int constellation_name_ok(const char *name) {
	size_t n = name ? strlen(name) : 0;
	if (n == 0 || n > 31 || name[0] == '-') {
		return 0;
	}
	for (size_t i = 0; i < n; i++) {
		char c = name[i];
		if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
				(c >= '0' && c <= '9') || c == '_' || c == '-')) {
			return 0;
		}
	}
	return 1;
}

void constellation_default(struct constellation *out) {
	memset(out, 0, sizeof(*out));
	strcpy(out->name, "default");
	out->nstars = 1;
	strcpy(out->stars[0].name, "star 0");
	out->stars[0].x = 960;
	out->stars[0].y = 540;
}

int constellation_add_app(struct cstar *s, const char *cmdline) {
	/* A newline would split the line in the file and corrupt the whole constellation. */
	if (!cmdline || !*cmdline || strlen(cmdline) >= sizeof(s->apps[0]) ||
			strpbrk(cmdline, "\r\n") || s->napps >= CSTAR_APPS_MAX) {
		return 0;
	}
	for (int i = 0; i < s->napps; i++) {
		if (strcmp(s->apps[i], cmdline) == 0) {
			return 0;
		}
	}
	strcpy(s->apps[s->napps++], cmdline);
	return 1;
}

int constellation_format(const struct constellation *c, char *buf, size_t n) {
	size_t used = 0;
	#define PUT(...) do { \
		int w = snprintf(buf + used, n - used, __VA_ARGS__); \
		if (w < 0 || (size_t)w >= n - used) return -1; \
		used += w; } while (0)
	PUT("constellation %s\n", c->name);
	for (int i = 0; i < c->nstars; i++) {
		const struct cstar *s = &c->stars[i];
		PUT("star %.17g %.17g %s\n", s->x, s->y, s->name);
		for (int j = 0; j < s->napps; j++) {
			PUT("app %s\n", s->apps[j]);
		}
	}
	PUT("end\n");
	#undef PUT
	return (int)used;
}

int constellation_parse(const char *text, struct constellation *out) {
	memset(out, 0, sizeof(*out));
	char *copy = strdup(text), *save = NULL;
	if (!copy) {
		return -1;
	}
	int have_header = 0, have_end = 0, ok = 1;
	for (char *line = strtok_r(copy, "\n", &save); line && ok;
			line = strtok_r(NULL, "\n", &save)) {
		if (have_end) { /* nothing may follow `end` */
			ok = 0;
		} else if (line[0] == '#' || line[0] == '\0') {
			continue;
		} else if (strncmp(line, "constellation ", 14) == 0 && !have_header) {
			if (!constellation_name_ok(line + 14)) { ok = 0; break; }
			snprintf(out->name, sizeof(out->name), "%s", line + 14);
			have_header = 1;
		} else if (strncmp(line, "star ", 5) == 0 && have_header) {
			double x, y;
			int off = 0;
			if (out->nstars >= CONST_STARS_MAX ||
					sscanf(line + 5, "%lf %lf %n", &x, &y, &off) < 2 || off == 0 ||
					line[5 + off] == '\0') {
				ok = 0;
				break;
			}
			struct cstar *s = &out->stars[out->nstars++];
			s->x = x;
			s->y = y;
			snprintf(s->name, sizeof(s->name), "%s", line + 5 + off);
		} else if (strncmp(line, "app ", 4) == 0 && out->nstars > 0) {
			constellation_add_app(&out->stars[out->nstars - 1], line + 4);
		} else if (strcmp(line, "end") == 0 && have_header) {
			have_end = 1;
		} else {
			ok = 0;
		}
	}
	free(copy);
	return (ok && have_header && have_end && out->nstars > 0) ? 0 : -1;
}

void constellation_config_dir(char *buf, size_t n) {
	const char *xdg = getenv("XDG_CONFIG_HOME"), *home = getenv("HOME");
	if (xdg && *xdg) {
		snprintf(buf, n, "%s/seiza", xdg);
	} else {
		snprintf(buf, n, "%s/.config/seiza", home ? home : ".");
	}
}

/* mkdir -p */
static int mkdir_p(const char *path) {
	char tmp[512];
	snprintf(tmp, sizeof(tmp), "%s", path);
	for (char *p = tmp + 1; *p; p++) {
		if (*p == '/') {
			*p = '\0';
			if (mkdir(tmp, 0755) != 0 && errno != EEXIST) {
				return -1;
			}
			*p = '/';
		}
	}
	return (mkdir(tmp, 0755) != 0 && errno != EEXIST) ? -1 : 0;
}

/* Write `text` to <dir>/<file> atomically: temp file in the same dir, then rename. */
static int write_atomic(const char *dir, const char *file, const char *text) {
	if (mkdir_p(dir) != 0) {
		return -1;
	}
	char tmp[512], dst[512];
	snprintf(tmp, sizeof(tmp), "%s/.%s.tmp", dir, file);
	snprintf(dst, sizeof(dst), "%s/%s", dir, file);
	FILE *f = fopen(tmp, "w");
	if (!f) {
		return -1;
	}
	int bad = fputs(text, f) < 0;
	bad |= fflush(f) != 0;
	bad |= fsync(fileno(f)) != 0;
	bad |= fclose(f) != 0;
	if (bad || rename(tmp, dst) != 0) {
		unlink(tmp);
		return -1;
	}
	return 0;
}

int constellation_save(const struct constellation *c, const char *dir) {
	if (!constellation_name_ok(c->name)) {
		return -1;
	}
	char buf[CONST_STARS_MAX * (64 + CSTAR_APPS_MAX * 270) + 128], file[64];
	if (constellation_format(c, buf, sizeof(buf)) < 0) {
		return -1;
	}
	snprintf(file, sizeof(file), "%s.const", c->name);
	return write_atomic(dir, file, buf);
}

int constellation_load(const char *dir, const char *name, struct constellation *out) {
	if (!constellation_name_ok(name)) {
		return -1;
	}
	char path[512], bad[520];
	snprintf(path, sizeof(path), "%s/%s.const", dir, name);
	FILE *f = fopen(path, "r");
	if (!f) {
		return -1;
	}
	static char buf[CONST_STARS_MAX * (64 + CSTAR_APPS_MAX * 270) + 128];
	size_t n = fread(buf, 1, sizeof(buf) - 1, f);
	fclose(f);
	buf[n] = '\0';
	if (constellation_parse(buf, out) != 0 || strcmp(out->name, name) != 0) {
		snprintf(bad, sizeof(bad), "%s.bad", path);
		rename(path, bad); /* keep it for yoru to look at */
		return -1;
	}
	return 0;
}

int constellation_set_last(const char *dir, const char *name) {
	if (!constellation_name_ok(name)) {
		return -1;
	}
	char line[40];
	snprintf(line, sizeof(line), "%s\n", name);
	return write_atomic(dir, "last", line);
}

int constellation_get_last(const char *dir, char *name, size_t n) {
	char path[512];
	snprintf(path, sizeof(path), "%s/last", dir);
	FILE *f = fopen(path, "r");
	if (!f) {
		return -1;
	}
	char line[64] = {0};
	char *got = fgets(line, sizeof(line), f);
	fclose(f);
	if (!got) {
		return -1;
	}
	line[strcspn(line, "\n")] = '\0';
	if (!constellation_name_ok(line) || strlen(line) >= n) {
		return -1;
	}
	strcpy(name, line);
	return 0;
}
