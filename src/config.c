struct frame FRAME; /* radius, border and glow: from the frame file, set by frame_colors_load() */
float BORDER_FOCUSED[4], BORDER_IDLE[4], GLOW_FOCUSED[4];

/* SEIZA: reading ~/.config/seiza: kdl, night, idle, frame colors, keys, autostart, hot reload. */
#include "seiza.h"

/* One part of ~/.config/seiza/seiza.kdl ("motion", "frame", "keys", "autostart") in the old flat
 * file format, from `seiza-conf export`, which also logs mistakes in the file to stderr. NULL when
 * there is no seiza.kdl or it says nothing about that part. Parsed after the old file, so it wins. */
char *cfg_kdl(const struct tinywl_server *server, const char *part) {
	static char text[65536];
	char path[512], cmd[64];
	snprintf(path, sizeof(path), "%s/seiza.kdl", server->cfg_dir);
	snprintf(cmd, sizeof(cmd), "seiza-conf export %s", part);
	FILE *p = access(path, R_OK) == 0 ? popen(cmd, "r") : NULL;
	if (!p) {
		return NULL;
	}
	size_t n = fread(text, 1, sizeof(text) - 1, p);
	pclose(p);
	text[n] = '\0';
	return n ? text : NULL;
}

struct wl_event_source *g_idle_timer;
int64_t g_last_input_ms;
static bool g_dimmed, g_locked_by_idle, g_off; /* what the idle clock has done since the last input */

int64_t now_ms(void) {
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

void outputs_power(struct tinywl_server *server, bool on) {
	struct tinywl_output *o;
	wl_list_for_each(o, &server->outputs, link) {
		struct wlr_output_state st;
		wlr_output_state_init(&st);
		wlr_output_state_set_enabled(&st, on);
		wlr_output_commit_state(o->wlr_output, &st);
		wlr_output_state_finish(&st);
		if (on) {
			wlr_output_schedule_frame(o->wlr_output);
		}
	}
}

/* Night light: the screens' colour follows the schedule in seiza.kdl (night { temp; from; to }), or `seiza night on|off|auto`.
 * The scene hands this colour table (g_night_tr) to the output: a real display applies it in hardware; whether a nested or headless one does is up to its backend. */
static double g_night_temp;                   /* kelvin at full warmth; 0 = no schedule */
static int g_night_from = 20 * 60, g_night_to = 7 * 60; /* minutes of the day */
int g_night_force = -1;                /* -1 follow the schedule, 0 off, 1 on */
static double g_night_last = 6500;            /* the temperature the screens were last set to */
struct wlr_color_transform *g_night_tr; /* NULL = colours untouched */
struct wl_event_source *g_night_timer;

void night_apply(struct tinywl_server *server) {
	double f = g_night_force >= 0 ? g_night_force : 0;
	if (g_night_force < 0 && g_night_temp > 0) {
		time_t now = time(NULL);
		struct tm tm;
		localtime_r(&now, &tm);
		f = night_factor(tm.tm_hour * 60 + tm.tm_min, g_night_from, g_night_to);
	}
	double warm = g_night_temp > 0 ? g_night_temp : 3500; /* `seiza night on` without a schedule */
	double k = 6500 + (warm - 6500) * f, m[3];
	if (k == g_night_last) {
		return; /* nothing changed (this also covers daylight with nothing to undo) */
	}
	g_night_last = k;
	night_rgb(k, m);
	if (g_night_tr) {
		wlr_color_transform_unref(g_night_tr);
		g_night_tr = NULL;
	}
	if (k < 6500) {
		/* Three 1D tables, like wlsunset's: a real display can take these straight into the hardware. */
		uint16_t lut[3][256];
		for (int c = 0; c < 3; c++) {
			for (int i = 0; i < 256; i++) {
				lut[c][i] = (uint16_t)lround(65535.0 * i / 255 * m[c]);
			}
		}
		g_night_tr = wlr_color_transform_init_lut_3x1d(256, lut[0], lut[1], lut[2]);
	}
	frames_all(server);
}

int night_tick(void *data) {
	night_apply(data);
	wl_event_source_timer_update(g_night_timer, 30000); /* the ramp is 30 minutes long: twice a minute is plenty */
	return 0;
}

/* Any key, pointer move, button or scroll: the idle clock starts over, the dim lifts and sleeping screens wake. */
void idle_activity(struct tinywl_server *server) {
	g_last_input_ms = now_ms();
	if (g_off) {
		outputs_power(server, true);
	}
	if (g_dimmed) {
		wlr_scene_node_set_enabled(&server->dim_rect->node, false);
	}
	g_dimmed = g_locked_by_idle = g_off = false;
}

int idle_tick(void *data) {
	struct tinywl_server *server = data;
	if (!wl_list_empty(&server->idle_inhibit->inhibitors)) {
		g_last_input_ms = now_ms(); /* shortcut: any inhibitor counts, even from a hidden window; upgrade if that bites */
	}
	int64_t idle = (now_ms() - g_last_input_ms) / 1000;
	if (server->idle_dim > 0 && idle >= server->idle_dim && !g_dimmed) {
		g_dimmed = true;
		wlr_scene_rect_set_size(server->dim_rect, server->full.width, server->full.height);
		wlr_scene_node_set_position(&server->dim_rect->node, server->full.x, server->full.y);
		wlr_scene_node_set_enabled(&server->dim_rect->node, true);
	}
	if (server->idle_lock > 0 && idle >= server->idle_lock && !g_locked_by_idle) {
		g_locked_by_idle = true;
		if (!server->locked) {
			op_spawn(server, "seiza-lock");
		}
	}
	if (server->idle_off > 0 && idle >= server->idle_off && !g_off) {
		g_off = true;
		outputs_power(server, false);
	}
	wl_event_source_timer_update(g_idle_timer, 1000);
	return 0;
}

/* Frame colors from the palette file (the star color, a dim mix of it with the
 * sky for unfocused windows, and the star color as a soft premultiplied bloom),
 * and how the camera moves, from the motion file. */
void frame_colors_load(struct tinywl_server *server) {
	struct palette p;
	palette_defaults(&p);
	char path[512];
	snprintf(path, sizeof(path), "%s/palette", server->cfg_dir);
	FILE *f = fopen(path, "r");
	if (f) {
		static char text[65536];
		size_t n = fread(text, 1, sizeof(text) - 1, f);
		fclose(f);
		text[n] = '\0';
		palette_parse(text, &p);
	}
	for (int i = 0; i < 3; i++) {
		BORDER_FOCUSED[i] = p.star[i];
		BORDER_IDLE[i] = p.sky[i] + (p.star[i] - p.sky[i]) * 0.3f;
		GLOW_FOCUSED[i] = p.star[i] * 0.30f;
	}
	BORDER_FOCUSED[3] = BORDER_IDLE[3] = 1.0f;
	GLOW_FOCUSED[3] = 0.30f;

	frame_defaults(&FRAME);
	snprintf(path, sizeof(path), "%s/frame", server->cfg_dir);
	if ((f = fopen(path, "r"))) {
		static char frame_text[65536];
		size_t n = fread(frame_text, 1, sizeof(frame_text) - 1, f);
		fclose(f);
		frame_text[n] = '\0';
		frame_parse(frame_text, &FRAME);
	}
	char *kdl = cfg_kdl(server, "frame");
	if (kdl) {
		frame_parse(kdl, &FRAME);
	}
	/* the bloom: off, or the star color at 0.3 (soft) or about twice that (bright), premultiplied */
	float strength = FRAME.glow == GLOW_OFF ? 0.0f : FRAME.glow == GLOW_BRIGHT ? 0.55f : 0.30f;
	for (int i = 0; i < 3; i++) {
		GLOW_FOCUSED[i] = p.star[i] * strength;
	}
	GLOW_FOCUSED[3] = strength;

	motion_defaults(&server->motion);
	snprintf(path, sizeof(path), "%s/motion", server->cfg_dir);
	if ((f = fopen(path, "r"))) {
		static char text[65536];
		size_t n = fread(text, 1, sizeof(text) - 1, f);
		fclose(f);
		text[n] = '\0';
		motion_parse(text, &server->motion);
	}
	if ((kdl = cfg_kdl(server, "motion"))) {
		motion_parse(kdl, &server->motion);
	}
	server->idle_dim = server->idle_lock = server->idle_off = 0;
	if ((kdl = cfg_kdl(server, "idle"))) {
		const char *dim = strstr(kdl, "dim="), *lock = strstr(kdl, "lock="), *off = strstr(kdl, "off=");
		if (dim) {
			sscanf(dim, "dim=%d", &server->idle_dim);
		}
		if (lock) {
			sscanf(lock, "lock=%d", &server->idle_lock);
		}
		if (off) {
			sscanf(off, "off=%d", &server->idle_off);
		}
	}
	g_night_temp = 0;
	g_night_from = 20 * 60;
	g_night_to = 7 * 60;
	if ((kdl = cfg_kdl(server, "night"))) {
		const char *t = strstr(kdl, "temp="), *f = strstr(kdl, "from="), *e = strstr(kdl, "to=");
		int h, m;
		if (t) {
			sscanf(t, "temp=%lf", &g_night_temp);
		}
		if (f && sscanf(f, "from=%d:%d", &h, &m) == 2 && h >= 0 && h < 24 && m >= 0 && m < 60) {
			g_night_from = h * 60 + m;
		}
		if (e && sscanf(e, "to=%d:%d", &h, &m) == 2 && h >= 0 && h < 24 && m >= 0 && m < 60) {
			g_night_to = h * 60 + m;
		}
	}
	night_apply(server);
	server->panel_corner = 1;
	server->panel_dwell = 150;
	if ((kdl = cfg_kdl(server, "panel"))) {
		const char *c = strstr(kdl, "corner="), *d = strstr(kdl, "dwell=");
		if (c) {
			server->panel_corner = !strncmp(c + 7, "off", 3) ? 0 : !strncmp(c + 7, "top-left", 8) ? 2 : !strncmp(c + 7, "bottom-right", 12) ? 3 : !strncmp(c + 7, "bottom-left", 11) ? 4 : 1;
		}
		if (d) {
			sscanf(d, "dwell=%d", &server->panel_dwell);
		}
	}
	if (g_idle_timer) {
		wl_event_source_timer_update(g_idle_timer, server->idle_dim > 0 || server->idle_lock > 0 || server->idle_off > 0 ? 1000 : 0);
	}
}

/* The live bindings, in the keys file's own syntax, for seiza-help to show: rewritten on every
 * (re)load, so the help never goes stale. */
void keys_dump(const struct keymap_cfg *k) {
	const char *dir = getenv("XDG_RUNTIME_DIR");
	char path[300];
	snprintf(path, sizeof(path), "%s/seiza-keys", dir ? dir : "/tmp");
	FILE *f = fopen(path, "w");
	if (!f) {
		return;
	}
	for (int i = 0; i < k->n; i++) {
		char name[64] = "?";
		xkb_keysym_get_name(k->b[i].sym, name, sizeof(name));
		fprintf(f, "%s%s%s%s%s = %s\n", k->b[i].mods & KMOD_SUPER ? "Super+" : "", k->b[i].mods & KMOD_CTRL ? "Ctrl+" : "",
			k->b[i].mods & KMOD_ALT ? "Alt+" : "", k->b[i].mods & KMOD_SHIFT ? "Shift+" : "", name, k->b[i].cmd);
	}
	fclose(f);
}


/* Defaults first, then the user's file on top. A missing file is fine. */
void keys_load(struct tinywl_server *server) {
	static struct keymap_cfg fresh;
	memset(&fresh, 0, sizeof(fresh));
	keys_parse(KEYS_DEFAULT, &fresh);
	char path[512];
	snprintf(path, sizeof(path), "%s/keys", server->cfg_dir);
	FILE *f = fopen(path, "r");
	if (f) {
		static char text[65536];
		size_t n = fread(text, 1, sizeof(text) - 1, f);
		fclose(f);
		text[n] = '\0';
		int bad = keys_parse(text, &fresh);
		if (bad > 0) {
			wlr_log(WLR_ERROR, "seiza: %d bad line(s) skipped in %s", bad, path);
		}
	}
	char kdl_path[512];
	snprintf(kdl_path, sizeof(kdl_path), "%s/seiza.kdl", server->cfg_dir);
	bool kdl_exists = access(kdl_path, R_OK) == 0;
	char *kdl = cfg_kdl(server, "keys");
	if (kdl) {
		keys_parse(kdl, &fresh);
	}
	server->keys = fresh;
	keys_dump(&fresh);
	if (kdl_exists) {
		op_spawn(server, "seiza-conf notify"); /* a desktop notification if seiza.kdl has mistakes */
	}
}


/* Programs to start with the session, one command per line in ~/.config/seiza/autostart
 * ('#' comments). With no such file the satellites that ship with Seiza start;
 * an empty file starts nothing. Only run at startup, never on reload-config. */
void autostart(struct tinywl_server *server) {
	static char text[8192] = "seiza-starfield\nseiza-bar\nseiza-widgets\nseiza-osd\nseiza-notify\nseiza-sleep\nwl-paste --type text --watch seiza-clip-store\nseiza-conf slideshow\n";
	char path[512];
	snprintf(path, sizeof(path), "%s/autostart", server->cfg_dir);
	FILE *f = fopen(path, "r");
	if (f) {
		size_t n = fread(text, 1, sizeof(text) - 1, f);
		text[n] = '\0';
		fclose(f);
	}
	char *kdl = cfg_kdl(server, "autostart"); /* an autostart block replaces the list */
	for (char *line = strtok(kdl ? kdl : text, "\n"); line; line = strtok(NULL, "\n")) {
		while (isspace((unsigned char)*line)) {
			line++;
		}
		if (*line && *line != '#') {
			op_spawn(server, line);
		}
	}
}

int op_reload_config(void *ud) {
	struct tinywl_server *server = ud;
	keys_load(server);
	rules_load(server);
	tile_load(server);
	frame_colors_load(server);
	relayout_all(server); /* a new radius or border changes every window */
	struct tinywl_output *out;
	wl_list_for_each(out, &server->outputs, link) {
		place_corners(out);
	}
	struct star *st;
	wl_list_for_each(st, &server->stars, link) {
		tile_star(server, st);
	}
	struct tinywl_toplevel *tl;
	wl_list_for_each(tl, &server->all_toplevels, all_link) {
		update_border(tl);
	}
	return 0;
}

/* Hot-reload: watch the config dir and run reload-config ~200 ms after the last write to a file
 * it reads. The dir is watched, not the files, because editors save by rename. */
static struct wl_event_source *g_reload_timer;

int reload_timer_fired(void *ud) {
	wlr_log(WLR_INFO, "seiza: config changed, reloading");
	op_reload_config(ud);
	return 0;
}

int config_changed(int fd, uint32_t mask, void *ud) {
	static const char *const watched[] = {"seiza.kdl", "keys", "palette", "frame", "motion"};
	char buf[4096] __attribute__((aligned(__alignof__(struct inotify_event))));
	ssize_t n = read(fd, buf, sizeof(buf));
	for (char *p = buf; n > 0 && p < buf + n; p += sizeof(struct inotify_event) + ((struct inotify_event *)p)->len) {
		struct inotify_event *ev = (struct inotify_event *)p;
		for (size_t i = 0; ev->len && i < sizeof(watched) / sizeof(*watched); i++) {
			if (strcmp(ev->name, watched[i]) == 0) {
				wl_event_source_timer_update(g_reload_timer, 200);
			}
		}
	}
	return 0;
}

void watch_config(struct tinywl_server *server) {
	struct wl_event_loop *loop = wl_display_get_event_loop(server->wl_display);
	int fd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
	if (fd < 0 || inotify_add_watch(fd, server->cfg_dir, IN_CLOSE_WRITE | IN_MOVED_TO) < 0) {
		wlr_log(WLR_ERROR, "seiza: no config hot-reload (inotify failed)");
		if (fd >= 0) {
			close(fd);
		}
		return;
	}
	g_reload_timer = wl_event_loop_add_timer(loop, reload_timer_fired, server);
	wl_event_loop_add_fd(loop, fd, WL_EVENT_READABLE, config_changed, server);
}
