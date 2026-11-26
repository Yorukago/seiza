/* SEIZA: the compositor. */
#include "seiza.h"

/* SEIZA window frame: a rounded plate behind the window (only its rim shows)
 * plus a soft gold bloom while focused. Thickness and radius are in screen px
 * at zoom 1; the radius follows the zoom, the thickness does not. */

/* A window is a Wayland one (xdg) or an X11 one (XWayland); the rest of the compositor asks these. */
struct wlr_surface *tl_surface(const struct tinywl_toplevel *tl) {
	return tl->xsurface ? tl->xsurface->surface : tl->xdg_toplevel->base->surface;
}

struct wlr_box tl_geometry(const struct tinywl_toplevel *tl) {
	if (tl->xsurface) {
		return (struct wlr_box){0, 0, tl->xsurface->width, tl->xsurface->height};
	}
	return tl->xdg_toplevel->base->geometry;
}

/* True once the client has something to configure: xdg is initialized, X11 is mapped. */
bool tl_ready(const struct tinywl_toplevel *tl) {
	if (tl->xsurface) {
		return tl->xsurface->surface && tl->xsurface->surface->mapped;
	}
	return tl->xdg_toplevel->base->initialized;
}

void tl_set_size(struct tinywl_toplevel *tl, int w, int h) {
	if (!tl->xsurface) {
		wlr_xdg_toplevel_set_size(tl->xdg_toplevel, w, h);
	} else if (w > 0 && h > 0) {
		wlr_xwayland_surface_configure(tl->xsurface, 0, 0, w, h);
	}
}

static void tl_close(struct tinywl_toplevel *tl) {
	if (tl->xsurface) {
		wlr_xwayland_surface_close(tl->xsurface);
	} else {
		wlr_xdg_toplevel_send_close(tl->xdg_toplevel);
	}
}

static void tl_activate(struct tinywl_toplevel *tl, bool on) {
	if (tl->xsurface) {
		wlr_xwayland_surface_activate(tl->xsurface, on);
	} else {
		wlr_xdg_toplevel_set_activated(tl->xdg_toplevel, on);
	}
}

void tl_set_maximized(struct tinywl_toplevel *tl, bool on) {
	if (tl->xsurface) {
		wlr_xwayland_surface_set_maximized(tl->xsurface, on, on);
	} else {
		wlr_xdg_toplevel_set_maximized(tl->xdg_toplevel, on);
	}
}

void tl_set_fullscreen(struct tinywl_toplevel *tl, bool on) {
	if (tl->xsurface) {
		wlr_xwayland_surface_set_fullscreen(tl->xsurface, on);
	} else {
		wlr_xdg_toplevel_set_fullscreen(tl->xdg_toplevel, on);
	}
}

struct wlr_surface *tl_surface_at(const struct tinywl_toplevel *tl, double x, double y, double *sx, double *sy) {
	if (tl->xsurface) {
		return tl->xsurface->surface ? wlr_surface_surface_at(tl->xsurface->surface, x, y, sx, sy) : NULL;
	}
	/* x,y start at the window geometry, but wlroots wants surface coordinates (a CSD shadow margin comes first) */
	struct wlr_box g = tl->xdg_toplevel->base->geometry;
	return wlr_xdg_surface_surface_at(tl->xdg_toplevel->base, x + g.x, y + g.y, sx, sy);
}

pid_t tl_pid(const struct tinywl_toplevel *tl) {
	pid_t pid = 0;
	if (tl->xsurface) {
		return tl->xsurface->pid;
	}
	wl_client_get_credentials(wl_resource_get_client(tl->xdg_toplevel->base->resource), &pid, NULL, NULL);
	return pid;
}

/* The window that had keyboard focus, whichever kind it is, is told it no longer does. */
void surface_deactivate(struct wlr_surface *surface) {
	struct wlr_xdg_toplevel *xdg = wlr_xdg_toplevel_try_from_wlr_surface(surface);
	struct wlr_xwayland_surface *x = wlr_xwayland_surface_try_from_wlr_surface(surface);
	if (xdg) {
		wlr_xdg_toplevel_set_activated(xdg, false);
	} else if (x) {
		wlr_xwayland_surface_activate(x, false);
	}
}

/* Fit the frame around the window's current geometry and color it by focus. */
static void frame_fit(struct wlr_scene_rect *border, struct wlr_scene_shadow *glow, const struct wlr_box *g,
		double z, bool shown, bool focused, float a) {
	int t = BORDER_W;
	int r = lround(WIN_RADIUS * z);
	int gx = 0, gy = 0; /* wlroots puts the geometry origin at the window's origin: a CSD shadow margin is cut off */
	int gw = lround(g->width * z), gh = lround(g->height * z);
	wlr_scene_node_set_enabled(&border->node, shown);
	wlr_scene_node_set_enabled(&glow->node, shown);
	wlr_scene_rect_set_size(border, gw + 2 * t, gh + 2 * t);
	wlr_scene_rect_set_corner_radius(border, r + t);
	float bc[4], gc[4]; /* colours are premultiplied, so alpha scales every channel */
	for (int i = 0; i < 4; i++) {
		bc[i] = (focused ? BORDER_FOCUSED : BORDER_IDLE)[i] * a;
		gc[i] = (focused ? GLOW_FOCUSED : GLOW_IDLE)[i] * a;
	}
	wlr_scene_rect_set_color(border, bc);
	wlr_scene_node_set_position(&border->node, gx - t, gy - t);
	int bloom = 2 * GLOW_SIGMA;
	wlr_scene_shadow_set_size(glow, gw + 2 * t + 2 * bloom, gh + 2 * t + 2 * bloom);
	wlr_scene_shadow_set_corner_radius(glow, r + t);
	wlr_scene_shadow_set_color(glow, gc);
	wlr_scene_node_set_position(&glow->node, gx - t - bloom, gy - t - bloom);
}

void update_border(struct tinywl_toplevel *tl) {
	struct wlr_box geo = tl_geometry(tl);
	bool focused = tl_surface(tl) && tl->server->seat->keyboard_state.focused_surface == tl_surface(tl);
	frame_fit(tl->border, tl->glow, &geo, tl->server->zoom * (1 - OPEN_POPIN * tl->faded), !tl->mode, focused,
		fminf(1.0f, fmaxf(0.0f, 1.0f - tl->faded)));
}

/* SEIZA stars. Territory size and grid spacing, in world units. New stars are
 * placed on a 3-column grid, left to right, then down. */
#define STAR_DOT 12    /* screen px, constant at any zoom */
#define STAR_HIT 24    /* screen px: how close a click must be to a star's dot */

static void screen_center(struct tinywl_server *server, double *x, double *y) {
	struct wlr_box b = {0};
	wlr_output_layout_get_box(server->output_layout, NULL, &b);
	*x = (b.width > 0 ? b.width : 1920) / 2.0;
	*y = (b.height > 0 ? b.height : 1080) / 2.0;
}

/* Place a star's dot for the current camera/zoom; brighten it if hovered. */
static void layout_star(struct tinywl_server *server, struct star *s) {
	double z = server->zoom;
	wlr_scene_node_set_enabled(&s->marker->node, server->overview);
	int cx = lround(s->cx * z), cy = lround(s->cy * z);
	float c[4] = {s->color[0], s->color[1], s->color[2], 1.0f};
	if (server->hover_star == s) {
		c[0] = c[1] = c[2] = 1.0f;
	}
	for (int i = 0; i < 4; i++) { /* the territory box is not drawn: the constellation view is dots */
		wlr_scene_node_set_enabled(&s->frame[i]->node, false);
	}
	wlr_scene_rect_set_size(s->dot, STAR_DOT, STAR_DOT);
	wlr_scene_node_set_position(&s->dot->node, cx - STAR_DOT / 2, cy - STAR_DOT / 2);
	wlr_scene_rect_set_color(s->dot, c);
}

/* Make a star at a world position. Its color follows its creation order. */
struct star *star_create_at(struct tinywl_server *server, const char *name,
		double cx, double cy) {
	static const float palette[6][3] = {
		{0.95f, 0.60f, 0.90f}, {0.55f, 0.80f, 0.95f}, {0.98f, 0.85f, 0.50f},
		{0.60f, 0.92f, 0.70f}, {0.85f, 0.65f, 0.98f}, {0.98f, 0.65f, 0.55f},
	};
	int n = wl_list_length(&server->stars);
	struct star *s = calloc(1, sizeof(*s));
	snprintf(s->name, sizeof(s->name), "%s", name);
	s->cx = cx;
	s->cy = cy;
	memcpy(s->color, palette[n % 6], sizeof(float) * 3);
	s->marker = wlr_scene_tree_create(server->canvas);
	float none[4] = {0};
	for (int i = 0; i < 4; i++) {
		s->frame[i] = wlr_scene_rect_create(s->marker, 0, 0, none);
	}
	s->dot = wlr_scene_rect_create(s->marker, 0, 0, none);
	wl_list_insert(server->stars.prev, &s->link);
	layout_star(server, s);
	return s;
}


/* The first free slot of the 3-column grid. */
int grid_slot(struct tinywl_server *server, double *cx, double *cy) {
	int slot = 0;
	for (;; slot++) {
		*cx = STAR_W / 2.0 + (slot % 3) * STAR_DX;
		*cy = STAR_H / 2.0 + (slot / 3) * STAR_DY;
		if (!star_at(server, *cx, *cy)) {
			return slot;
		}
	}
}

/* A name no star has yet: "star N" for the first N that is free. */
void star_new_name(struct tinywl_server *server, char name[32]) {
	for (int n = 0;; n++) {
		snprintf(name, 32, "star %d", n);
		struct star *s;
		bool taken = false;
		wl_list_for_each(s, &server->stars, link) {
			taken |= strcmp(s->name, name) == 0;
		}
		if (!taken) {
			return;
		}
	}
}

/* Make a new star in the first free slot of the grid. */
struct star *star_create(struct tinywl_server *server) {
	double cx, cy;
	int slot = grid_slot(server, &cx, &cy);
	char name[32];
	snprintf(name, sizeof(name), "star %d", slot);
	return star_create_at(server, name, cx, cy);
}

/* Which star's territory contains this world point, if any. */
struct star *star_at(struct tinywl_server *server, double wx, double wy) {
	struct star *s;
	wl_list_for_each(s, &server->stars, link) {
		if (fabs(wx - s->cx) <= STAR_W / 2.0 && fabs(wy - s->cy) <= STAR_H / 2.0) {
			return s;
		}
	}
	return NULL;
}

/* SEIZA persistence: the active constellation is rewritten whenever its
 * structure changes (never per frame). A failed save is logged, not fatal. */
int persist_write(struct tinywl_server *server, const char *name, bool make_last) {
	struct constellation c = {0};
	snprintf(c.name, sizeof(c.name), "%s", name);
	struct star *s;
	wl_list_for_each(s, &server->stars, link) {
		if (c.nstars >= CONST_STARS_MAX) {
			break;
		}
		struct cstar *cs = &c.stars[c.nstars++];
		snprintf(cs->name, sizeof(cs->name), "%s", s->name);
		cs->x = s->cx;
		cs->y = s->cy;
		cs->napps = s->napps;
		cs->tiled = s->tiled;
		/* (links are copied below) */
		memcpy(cs->apps, s->apps, sizeof(cs->apps));
	}
	memcpy(c.links, server->links, sizeof(c.links));
	c.nlinks = server->nlinks;
	char last[32];
	bool last_changed = make_last && (constellation_get_last(server->cfg_dir, last, sizeof(last)) != 0 ||
		strcmp(last, c.name) != 0); /* avoids a second fsync on every autosave */
	if (constellation_save(&c, server->cfg_dir) != 0 ||
			(last_changed && constellation_set_last(server->cfg_dir, c.name) != 0)) {
		wlr_log(WLR_ERROR, "seiza: could not save constellation '%s' in %s",
			c.name, server->cfg_dir);
		return -1;
	}
	return 0;
}

void persist_save(struct tinywl_server *server) {
	persist_write(server, server->constellation_name, true);
}

/* Build the stars of `c` and make the first one current. */
void persist_apply(struct tinywl_server *server, const struct constellation *c) {
	snprintf(server->constellation_name, sizeof(server->constellation_name), "%s", c->name);
	memcpy(server->links, c->links, sizeof(server->links));
	server->nlinks = c->nlinks;
	for (int i = 0; i < c->nstars; i++) {
		struct star *s = star_create_at(server, c->stars[i].name, c->stars[i].x, c->stars[i].y);
		s->napps = c->stars[i].napps;
		s->tiled = c->stars[i].tiled;
		memcpy(s->apps, c->stars[i].apps, sizeof(s->apps));
	}
	server->current_star = wl_container_of(server->stars.next, server->current_star, link);
	/* An older file named a figure and the point each star held: turn that into threads. */
	struct figure f;
	if (c->nlinks == 0 && c->figure[0] && figure_find(server->cfg_dir, c->figure, &f) == 0) {
		for (int l = 0; l < f.nlinks && server->nlinks < CONST_LINKS_MAX; l++) {
			int ends[2] = {-1, -1};
			for (int i = 0; i < c->nstars; i++) {
				for (int e = 0; e < 2; e++) {
					if (c->stars[i].fig_point == f.links[l][e] + 1) {
						ends[e] = i;
					}
				}
			}
			if (ends[0] >= 0 && ends[1] >= 0) {
				server->links[server->nlinks][0] = ends[0];
				server->links[server->nlinks++][1] = ends[1];
			}
		}
	}
}

/* Startup: open the constellation named in `last`, else the default one. */
static void persist_startup(struct tinywl_server *server) {
	struct constellation c;
	char name[32];
	constellation_config_dir(server->cfg_dir, sizeof(server->cfg_dir));
	if (constellation_get_last(server->cfg_dir, name, sizeof(name)) != 0 ||
			constellation_load(server->cfg_dir, name, &c) != 0) {
		/* Don't start a blank 'default' over a real saved one: autosave would overwrite it. */
		if (constellation_load(server->cfg_dir, "default", &c) != 0) {
			wlr_log(WLR_INFO, "seiza: no usable saved constellation, starting 'default'");
			constellation_default(&c);
		}
	}
	persist_apply(server, &c);
}

/* Append `s` to buf as a JSON string. */
static size_t json_str(char *buf, size_t n, size_t at, const char *s) {
	at += snprintf(buf + at, n - at, "\"");
	for (; *s && at + 8 < n; s++) {
		if (*s == '"' || *s == '\\') {
			buf[at++] = '\\';
		}
		if ((unsigned char)*s >= 0x20) {
			buf[at++] = *s;
		}
	}
	buf[at] = '\0';
	return at + snprintf(buf + at, n - at, "\"");
}

/* Tell subscribers (satellites) the state they draw from. Screen position of a
 * world point is  world * zoom - cam;  star boxes are centered on x,y. Call
 * whenever any of this may have changed: ipc_publish drops repeats. */
void publish_state(struct tinywl_server *server) {
	static char buf[16384];
	double sw, sh;
	screen_center(server, &sw, &sh);
	size_t at = snprintf(buf, sizeof(buf), "{\"constellation\":");
	at = json_str(buf, sizeof(buf), at, server->constellation_name);
	at += snprintf(buf + at, sizeof(buf) - at, ",\"current\":");
	at = json_str(buf, sizeof(buf), at, server->current_star ? server->current_star->name : "");
	at += snprintf(buf + at, sizeof(buf) - at,
		",\"overview\":%s,\"zoom\":%.4f,\"cam\":[%.1f,%.1f],\"screen\":[%.0f,%.0f],\"stars\":[",
		server->overview ? "true" : "false", server->zoom, server->cam_x, server->cam_y, sw * 2, sh * 2);
	struct star *st;
	wl_list_for_each(st, &server->stars, link) {
		int windows = 0;
		struct tinywl_toplevel *tl;
		wl_list_for_each(tl, &server->toplevels, link) {
			windows += tl->star == st;
		}
		at += snprintf(buf + at, sizeof(buf) - at, "%s{\"name\":", st->link.prev == &server->stars ? "" : ",");
		at = json_str(buf, sizeof(buf), at, st->name);
		at += snprintf(buf + at, sizeof(buf) - at, ",\"x\":%.0f,\"y\":%.0f,\"w\":%d,\"h\":%d,\"windows\":%d}",
			st->cx, st->cy, STAR_W, STAR_H, windows);
		if (at + 200 >= sizeof(buf)) {
			return; /* does not fit: keep the last good state rather than send half */
		}
	}
	/* threads, as pairs of indexes into `stars` */
	at += snprintf(buf + at, sizeof(buf) - at, "],\"links\":[");
	for (int l = 0; l < server->nlinks && at + 40 < sizeof(buf); l++) {
		at += snprintf(buf + at, sizeof(buf) - at, "%s[%d,%d]", l ? "," : "", server->links[l][0], server->links[l][1]);
	}
	snprintf(buf + at, sizeof(buf) - at, "]}");
	ipc_publish(buf);
}

/* The current star follows the camera: whichever star's territory is under the
 * screen center. In empty space between stars it stays as it was. */
void update_current_star(struct tinywl_server *server) {
	double sx, sy;
	screen_center(server, &sx, &sy);
	struct star *s = star_at(server, (sx + server->cam_x) / server->zoom,
		(sy + server->cam_y) / server->zoom);
	if (s && s != server->current_star) {
		server->current_star = s;
		wlr_log(WLR_INFO, "seiza: now in %s", s->name);
		publish_state(server);
	}
}

/* SEIZA limits. Past ~2x the stretched pixels turn grainy, below ~0.25x
 * windows become unreadable specks. Window sizes are in world units. */
#define ZOOM_MIN 0.25
#define ZOOM_MAX 2.0

/* SEIZA zoom. wlroots has no "scale this subtree", so each surface buffer is
 * drawn at a scaled size. wlroots resets that size on every client commit, so
 * this is re-applied from xdg_toplevel_commit.
 * shortcut: subsurface/popup buffers get scaled but not their offsets, and a
 * nonzero xdg geometry offset (CSD shadows) is not scaled either; fix when a
 * real app needs it. */
struct scale_arg {
	double z;
	int radius;
	float opacity;
	struct wlr_surface *root; /* an xdg window's main surface: layout_toplevel clips it to the window geometry */
	struct wlr_box geo;
};

#define OPEN_SECS 0.3 /* Hyprland's speed 3 (units of 100 ms), as in the user's windowsIn */

#define OPEN_CURVE "expressiveFastSpatial" /* the open animation's curve; emphasizedDecel is the calm one */

/* How far along its open animation a window is: 0 at the start, 1 at the end, past 1 while it bounces (the end clears open_t0). */
static float open_alpha(struct tinywl_toplevel *tl) {
	if (tl->open_t0 == 0) {
		return 1.0f;
	}
	double p = (mono_s() - tl->open_t0) / OPEN_SECS;
	if (p >= 1.0) {
		tl->open_t0 = 0;
		return 1.0f;
	}
	return (float)motion_bezier(motion_curve(OPEN_CURVE), p); /* may pass 1.0: the window overshoots */
}

static void scale_buffer(struct wlr_scene_buffer *sb, int sx, int sy, void *data) {
	struct scale_arg *arg = data;
	double z = arg->z;
	struct wlr_scene_surface *ss = wlr_scene_surface_try_from_buffer(sb);
	if (!ss) {
		return;
	}
	/* The unscaled size wlroots itself uses (surface.c: surface_reconfigure): the surface size,
	 * or the window geometry for an xdg main surface (its CSD shadow is clipped away). */
	int w = ss->surface->current.width, h = ss->surface->current.height;
	if (ss->surface == arg->root && arg->geo.width > 0 && arg->geo.height > 0) {
		w = arg->geo.width;
		h = arg->geo.height;
	}
	if (w <= 0 || h <= 0) {
		return;
	}
	wlr_scene_buffer_set_dest_size(sb, fmax(1, lround(w * z)), fmax(1, lround(h * z)));
	wlr_scene_buffer_set_corner_radius(sb, arg->radius);
	wlr_scene_buffer_set_opacity(sb, arg->opacity);
}

/* Put one window where world + camera + zoom say it should be. */
void layout_toplevel(struct tinywl_toplevel *tl) {
	float alpha = open_alpha(tl);
	tl->faded = 1.0f - alpha;
	double z = tl->server->zoom, pz = z * (1 - OPEN_POPIN * tl->faded); /* pz: zoom while popping in */
	/* The constellation view shows no windows, and a window that is not on screen (an X11 program's
	 * helper windows exist long before, or without ever, being shown) gets no frame either. */
	struct wlr_surface *surface = tl_surface(tl);
	wlr_scene_node_set_enabled(&tl->scene_tree->node, surface && surface->mapped && !tl->server->overview);
	if (tl->star) {
		/* a window can be anywhere on the canvas, but its top edge stays below the bar (usable.y is 0 until a bar exists) */
		tl->wy = fmax(tl->wy, tl->star->cy - STAR_H / 2.0 + (tl->mode ? 0 : tl->server->usable.y - tl->server->full.y));
	}
	struct wlr_box geo = tl_geometry(tl); /* the smaller window keeps its centre */
	if (!tl->xsurface) {
		wlr_scene_subsurface_tree_set_clip(&tl->scene_tree->node, &geo); /* cut off a CSD shadow margin */
	}
	wlr_scene_node_set_position(&tl->scene_tree->node, lround(tl->wx * z + (z - pz) * geo.width / 2), lround(tl->wy * z + (z - pz) * geo.height / 2));
	if (tl->xsurface && surface && surface->mapped && tl->xsurface->width && tl->xsurface->height &&
			(tl->xsurface->x != 0 || tl->xsurface->y != 0)) {
		/* Every X11 window sits at X position (0,0), whatever its place on our canvas: X only delivers
		 * the pointer to a window inside its own screen (the size of the output), and the canvas is far
		 * bigger than that. Which window gets the pointer follows Wayland pointer focus, not X geometry. */
		wlr_xwayland_surface_configure(tl->xsurface, 0, 0, tl->xsurface->width, tl->xsurface->height);
	}
	struct scale_arg arg = {pz, tl->mode ? 0 : lround(WIN_RADIUS * pz), fminf(1.0f, fmaxf(0.0f, alpha)),
		tl->xsurface ? NULL : surface, geo}; /* maximized windows have square corners */
	wlr_scene_node_for_each_buffer(&tl->scene_tree->node, scale_buffer, &arg);
	update_border(tl);
}

void layout_unmanaged(struct xunmanaged *u) {
	double z = u->server->zoom;
	wlr_scene_node_set_position(&u->tree->node, lround((u->ax + u->xs->x) * z), lround((u->ay + u->xs->y) * z));
	struct scale_arg arg = {z, 0, 1.0f};
	wlr_scene_node_for_each_buffer(&u->tree->node, scale_buffer, &arg);
}

/* Re-place the canvas and every window; call after the camera or zoom changes. */
void relayout_all(struct tinywl_server *server) {
	/* The screen center may stray one screen width/height outside the stars' bounding box. */
	if (!wl_list_empty(&server->stars)) {
		double x0 = 1e18, y0 = 1e18, x1 = -1e18, y1 = -1e18, sx, sy;
		struct star *st;
		wl_list_for_each(st, &server->stars, link) {
			x0 = fmin(x0, st->cx - STAR_W / 2.0); x1 = fmax(x1, st->cx + STAR_W / 2.0);
			y0 = fmin(y0, st->cy - STAR_H / 2.0); y1 = fmax(y1, st->cy + STAR_H / 2.0);
		}
		screen_center(server, &sx, &sy);
		double z = server->zoom;
		server->cam_x = fmin(fmax(server->cam_x, (x0 - 2 * sx / z) * z - sx), (x1 + 2 * sx / z) * z - sx);
		server->cam_y = fmin(fmax(server->cam_y, (y0 - 2 * sy / z) * z - sy), (y1 + 2 * sy / z) * z - sy);
	}
	wlr_scene_node_set_position(&server->canvas->node,
		lround(-server->cam_x), lround(-server->cam_y));
	struct tinywl_toplevel *tl;
	wl_list_for_each(tl, &server->toplevels, link) {
		layout_toplevel(tl);
	}
	struct star *s;
	wl_list_for_each(s, &server->stars, link) {
		layout_star(server, s);
	}
	struct xunmanaged *um;
	wl_list_for_each(um, &server->unmanaged, link) {
		layout_unmanaged(um);
	}
	publish_state(server);
}


/* Multiply the zoom by `factor`, keeping the world point under the cursor
 * fixed on screen (so it zooms toward the pointer, like a map). */
void zoom_at(struct tinywl_server *server, double factor) {
	if (server->overview) {
		return; /* the overview has its own fixed fit; leave it with Super+O */
	}
	anim_cancel(server);
	double z = fmin(ZOOM_MAX, fmax(ZOOM_MIN, server->zoom * factor));
	factor = z / server->zoom;
	server->cam_x = (server->cursor->x + server->cam_x) * factor - server->cursor->x;
	server->cam_y = (server->cursor->y + server->cam_y) * factor - server->cursor->y;
	server->zoom = z;
	relayout_all(server);
}

/* SEIZA camera glides ("pan up" into the constellation, "pan down" into a star):
 * ease over the motion file's pan_secs, moving the world point at the screen centre in
 * a straight line and the zoom in log space, so zooming looks even. */

double mono_s(void) {
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec + ts.tv_nsec / 1e9;
}

void anim_cancel(struct tinywl_server *server) {
	server->anim.on = false;
}

/* One step of the glide, to the camera the clock says it should have now. It runs
 * from output_frame, so every frame shown has exactly one step in it: a separate
 * timer drifts against the screen's refresh and makes the glide stutter. */
void anim_step(struct tinywl_server *server) {
	if (!server->anim.on) {
		return;
	}
	double p = fmin(1.0, (mono_s() - server->anim.t0) / server->anim.dur);
	double e = motion_ease(&server->motion, p);
	double z = exp(log(server->anim.z0) + (log(server->anim.z1) - log(server->anim.z0)) * e);
	double wx = server->anim.cx0 + (server->anim.cx1 - server->anim.cx0) * e;
	double wy = server->anim.cy0 + (server->anim.cy1 - server->anim.cy0) * e;
	double sx, sy;
	screen_center(server, &sx, &sy);
	server->zoom = z;
	server->cam_x = wx * z - sx;
	server->cam_y = wy * z - sy;
	relayout_all(server);
	if (p >= 1.0) {
		server->anim.on = false;
	}
}

void frames_all(struct tinywl_server *server) {
	struct tinywl_output *o;
	wl_list_for_each(o, &server->outputs, link) {
		wlr_output_schedule_frame(o->wlr_output);
	}
}

/* One step of every window that is still fading in; true while any is. */
bool open_step(struct tinywl_server *server) {
	bool any = false;
	struct tinywl_toplevel *tl;
	wl_list_for_each(tl, &server->toplevels, link) {
		if (tl->open_t0) {
			layout_toplevel(tl);
			any |= tl->open_t0 != 0;
		}
	}
	return any;
}

/* A closed window lingers as a "ghost": its last frame, fading and shrinking like the open animation
 * played backwards. It owns its own nodes, because the client destroys the window right after it unmaps. */
#define CLOSE_SECS 0.2
#define CLOSE_CURVE "emphasizedAccel"
struct ghost {
	struct wl_list link;
	struct wlr_scene_tree *tree;
	struct wlr_scene_rect *border;
	struct wlr_scene_shadow *glow;
	struct wlr_scene_buffer *buf;
	double wx, wy, t0;
	struct wlr_box geo;
	int bx, by, sw, sh, mode; /* where the surface sits in the window, its unscaled size, maximized or not */
	bool focused;
};

struct grab_arg {
	struct wlr_surface *surface;
	struct wlr_scene_buffer *sb;
	int x, y;
};

static void grab_buffer(struct wlr_scene_buffer *sb, int sx, int sy, void *data) {
	struct grab_arg *a = data;
	struct wlr_scene_surface *ss = wlr_scene_surface_try_from_buffer(sb);
	if (!a->sb && ss && ss->surface == a->surface && sb->buffer) {
		a->sb = sb, a->x = sx, a->y = sy;
	}
}

/* Called as a window unmaps, while its scene still shows the last frame. */
void ghost_start(struct tinywl_toplevel *tl) {
	struct wlr_surface *surface = tl_surface(tl);
	if (!surface || !surface->mapped || tl->server->motion.reduce || tl->server->overview) {
		return;
	}
	struct grab_arg a = {surface, NULL, 0, 0};
	wlr_scene_node_for_each_buffer(&tl->scene_tree->node, grab_buffer, &a);
	if (!a.sb || surface->current.width <= 0 || surface->current.height <= 0) {
		return;
	}
	struct ghost *g = calloc(1, sizeof(*g));
	g->tree = wlr_scene_tree_create(tl->server->canvas);
	g->glow = wlr_scene_shadow_create(g->tree, 0, 0, WIN_RADIUS, GLOW_SIGMA, GLOW_IDLE);
	g->border = wlr_scene_rect_create(g->tree, 0, 0, BORDER_IDLE);
	g->buf = wlr_scene_buffer_create(g->tree, a.sb->buffer); /* locks the buffer: it outlives the client's */
	wlr_scene_buffer_set_source_box(g->buf, &a.sb->src_box);
	wlr_scene_buffer_set_transform(g->buf, a.sb->transform);
	g->wx = tl->wx, g->wy = tl->wy, g->t0 = mono_s();
	g->geo = tl_geometry(tl);
	g->bx = a.x, g->by = a.y, g->mode = tl->mode;
	g->sw = surface->current.width, g->sh = surface->current.height;
	g->focused = tl->server->seat->keyboard_state.focused_surface == surface;
	wl_list_insert(&tl->server->ghosts, &g->link);
	frames_all(tl->server);
}

static void ghost_free(struct ghost *g) {
	wlr_scene_node_destroy(&g->tree->node);
	wl_list_remove(&g->link);
	free(g);
}

/* One step of every closing ghost; true while any is left. */
bool ghost_step(struct tinywl_server *server) {
	bool any = false;
	struct ghost *g, *tmp;
	wl_list_for_each_safe(g, tmp, &server->ghosts, link) {
		double p = (mono_s() - g->t0) / CLOSE_SECS;
		if (p >= 1.0 || server->overview) {
			ghost_free(g);
			continue;
		}
		any = true;
		double e = fmin(1.0, fmax(0.0, motion_bezier(motion_curve(CLOSE_CURVE), p)));
		double z = server->zoom, pz = z * (1 - OPEN_POPIN * e);
		wlr_scene_node_set_position(&g->tree->node, lround(g->wx * z + (z - pz) * g->geo.width / 2),
			lround(g->wy * z + (z - pz) * g->geo.height / 2));
		wlr_scene_node_set_position(&g->buf->node, g->bx, g->by);
		wlr_scene_buffer_set_dest_size(g->buf, fmax(1, lround(g->sw * pz)), fmax(1, lround(g->sh * pz)));
		wlr_scene_buffer_set_corner_radius(g->buf, g->mode ? 0 : lround(WIN_RADIUS * pz));
		wlr_scene_buffer_set_opacity(g->buf, 1.0f - (float)e);
		frame_fit(g->border, g->glow, &g->geo, pz, !g->mode, g->focused, 1.0f - (float)e);
	}
	return any;
}

static void anim_to(struct tinywl_server *server, double cam_x, double cam_y, double zoom) {
	double sx, sy;
	screen_center(server, &sx, &sy);
	server->anim.cx0 = (server->cam_x + sx) / server->zoom;
	server->anim.cy0 = (server->cam_y + sy) / server->zoom;
	server->anim.z0 = server->zoom;
	server->anim.cx1 = (cam_x + sx) / zoom;
	server->anim.cy1 = (cam_y + sy) / zoom;
	server->anim.z1 = zoom;
	server->anim.t0 = mono_s();
	server->anim.dur = motion_duration(&server->motion);
	server->anim.on = true;
	frames_all(server); /* start the frames that will carry it */
}

/* The constellation view hides the bar (layer Top), the widgets (layer Bottom) and the windows. */
static void set_overview(struct tinywl_server *server, bool on) {
	server->overview = on;
	wlr_scene_node_set_enabled(&server->layers[2]->node, !on);
	wlr_scene_node_set_enabled(&server->layers[1]->node, !on);
}

/* Glide into a star at 1:1 zoom and make it the star new windows land in. */
void fly_to_star(struct tinywl_server *server, struct star *s) {
	double sx, sy;
	screen_center(server, &sx, &sy);
	server->current_star = s;
	set_overview(server, false);
	server->hover_star = NULL;
	wlr_log(WLR_INFO, "seiza: entered %s", s->name);
	anim_to(server, s->cx - sx, s->cy - sy, 1.0);
	relayout_all(server); /* windows reappear and the bar returns while it glides */
}

/* Zoom out so every star fits on screen and show the star markers. This
 * ignores ZOOM_MIN on purpose: fitting the whole constellation needs more.
 * shortcut: fit uses star centers + territory, so it is wrong if the output
 * is a different aspect ratio than 16:9, fine for the spike. */
void enter_overview(struct tinywl_server *server) {
	double minx = 1e18, miny = 1e18, maxx = -1e18, maxy = -1e18;
	struct star *s;
	wl_list_for_each(s, &server->stars, link) {
		minx = fmin(minx, s->cx - STAR_W / 2.0);
		maxx = fmax(maxx, s->cx + STAR_W / 2.0);
		miny = fmin(miny, s->cy - STAR_H / 2.0);
		maxy = fmax(maxy, s->cy + STAR_H / 2.0);
	}
	double sx, sy;
	screen_center(server, &sx, &sy);
	double z = 0.9 * fmin(2 * sx / (maxx - minx), 2 * sy / (maxy - miny));
	z = fmin(1.0, fmax(0.05, z));
	set_overview(server, true);
	wl_list_for_each(s, &server->stars, link) {
		wlr_scene_node_raise_to_top(&s->marker->node);
	}
	wlr_seat_pointer_clear_focus(server->seat);
	anim_to(server, (minx + maxx) / 2 * z - sx, (miny + maxy) / 2 * z - sy, z);
	relayout_all(server);
}

/* The star whose dot is under a screen point, if any. */
struct star *star_dot_at(struct tinywl_server *server, double x, double y) {
	struct star *s, *best = NULL;
	double best_d = STAR_HIT;
	wl_list_for_each(s, &server->stars, link) {
		double d = hypot(s->cx * server->zoom - server->cam_x - x, s->cy * server->zoom - server->cam_y - y);
		if (d < best_d) {
			best_d = d;
			best = s;
		}
	}
	return best;
}

/* Overview pointer handling: highlight the star under the cursor. */
void overview_pointer(struct tinywl_server *server) {
	struct star *s = star_dot_at(server, server->cursor->x, server->cursor->y);
	if (s != server->hover_star) {
		server->hover_star = s;
		relayout_all(server); /* recolors the hovered star */
	}
}

bool main_mod_held(struct tinywl_server *server) {
	struct wlr_keyboard *kb = wlr_seat_get_keyboard(server->seat);
	return kb && (wlr_keyboard_get_modifiers(kb) & WLR_MODIFIER_LOGO);
}

void focus_toplevel(struct tinywl_toplevel *toplevel) {
	/* Note: this function only deals with keyboard focus. */
	if (toplevel == NULL || toplevel->server->locked) {
		return;
	}
	struct tinywl_server *server = toplevel->server;
	struct wlr_seat *seat = server->seat;
	struct wlr_surface *prev_surface = seat->keyboard_state.focused_surface;
	struct wlr_surface *surface = tl_surface(toplevel);
	if (!surface || prev_surface == surface) {
		/* Don't re-focus an already focused surface. */
		return;
	}
	if (prev_surface) {
		/*
		 * Deactivate the previously focused surface. This lets the client know
		 * it no longer has focus and the client will repaint accordingly, e.g.
		 * stop displaying a caret.
		 */
		surface_deactivate(prev_surface);
	}
	struct wlr_keyboard *keyboard = wlr_seat_get_keyboard(seat);
	/* Move the toplevel to the front */
	wlr_scene_node_raise_to_top(&toplevel->scene_tree->node);
	wlr_scene_node_raise_to_top(&server->xtop->node); /* X11 menus stay above the windows */
	wl_list_remove(&toplevel->link);
	wl_list_insert(&server->toplevels, &toplevel->link);
	/* Activate the new surface */
	tl_activate(toplevel, true);
	if (toplevel->xsurface) { /* X keeps its own stacking order: keep it the same as ours */
		wlr_xwayland_surface_restack(toplevel->xsurface, NULL, XCB_STACK_MODE_ABOVE);
	}
	/*
	 * Tell the seat to have the keyboard enter this surface. wlroots will keep
	 * track of this and automatically send key events to the appropriate
	 * clients without additional work on your part.
	 */
	if (keyboard != NULL) {
		wlr_seat_keyboard_notify_enter(seat, surface,
			keyboard->keycodes, keyboard->num_keycodes, &keyboard->modifiers);
	}
	/* SEIZA: recolor every frame now that focus moved. */
	struct tinywl_toplevel *tl;
	wl_list_for_each(tl, &server->toplevels, link) {
		update_border(tl);
	}
}

void keyboard_handle_modifiers(
		struct wl_listener *listener, void *data) {
	/* This event is raised when a modifier key, such as shift or alt, is
	 * pressed. We simply communicate this to the client. */
	struct tinywl_keyboard *keyboard =
		wl_container_of(listener, keyboard, modifiers);
	/*
	 * A seat can only have one keyboard, but this is a limitation of the
	 * Wayland protocol - not wlroots. We assign all connected keyboards to the
	 * same seat. You can swap out the underlying wlr_keyboard like this and
	 * wlr_seat handles this transparently.
	 */
	wlr_seat_set_keyboard(keyboard->server->seat, keyboard->wlr_keyboard);
	/* Send modifiers to the client. */
	wlr_seat_keyboard_notify_modifiers(keyboard->server->seat,
		&keyboard->wlr_keyboard->modifiers);
}

/* SEIZA: slide the camera by (dx, dy) screen pixels. */
static void pan_camera(struct tinywl_server *server, double dx, double dy) {
	anim_cancel(server);
	server->cam_x += dx;
	server->cam_y += dy;
	relayout_all(server);
	update_current_star(server);
}

void op_quit(void *ud) {
	struct tinywl_server *server = ud;
	wl_display_terminate(server->wl_display);
}

static void op_pan(void *ud, int dx, int dy) { pan_camera(ud, dx, dy); }

/* Reset zoom to 1:1, keeping the screen center fixed. */
static void op_zoom_reset(void *ud) {
	struct tinywl_server *server = ud;
	if (server->overview) {
		return;
	}
	anim_cancel(server);
	double sx, sy;
	screen_center(server, &sx, &sy);
	server->cam_x = (server->cam_x + sx) / server->zoom - sx;
	server->cam_y = (server->cam_y + sy) / server->zoom - sy;
	server->zoom = 1.0;
	relayout_all(server);
}

static void op_cycle_window(void *ud) {
	struct tinywl_server *server = ud;
	if (wl_list_length(&server->toplevels) < 2) {
		return;
	}
	struct tinywl_toplevel *next_toplevel =
		wl_container_of(server->toplevels.prev, next_toplevel, link);
	focus_toplevel(next_toplevel);
}

/* Reap every launched program that has exited (no zombies). */
static int reap_children(int signo, void *data) {
	(void)signo; (void)data;
	while (waitpid(-1, NULL, WNOHANG) > 0) {
	}
	return 0;
}

/* Run through the configured shell, else ysh if installed, else /bin/sh. */
void op_spawn(void *ud, const char *cmdline) {
	struct tinywl_server *server = ud;
	if (fork() == 0) {
		const char *shell = server->keys.shell[0] ? server->keys.shell : "ysh";
		execlp(shell, shell, "-c", cmdline, (void *)NULL);
		execl("/bin/sh", "/bin/sh", "-c", cmdline, (void *)NULL);
		_exit(127);
	}
}

/* The only thing that ever launches remembered apps: an explicit request. */
static int op_relaunch(void *ud) {
	struct tinywl_server *server = ud;
	struct star *s = server->current_star;
	wlr_log(WLR_INFO, "seiza: relaunching %d app(s) for %s", s->napps, s->name);
	for (int i = 0; i < s->napps; i++) {
		op_spawn(server, s->apps[i]);
	}
	return 0;
}
/* Keyboard window ops act on the focused window (head of the toplevels list). */
static struct tinywl_toplevel *focused_toplevel(struct tinywl_server *server) {
	if (wl_list_empty(&server->toplevels)) {
		return NULL;
	}
	return wl_container_of(server->toplevels.next, (struct tinywl_toplevel *)NULL, link);
}

/* Mode 1 (maximized) fills what the bar leaves over, inside the window's star; mode 2 (fullscreen, asked for by the program)
 * fills the whole output and hides the bar while that star is in view (output_frame). The window's old place is remembered
 * and comes back on mode 0. Dragging or resizing a maximized or fullscreen window un-maximizes it (begin_interactive). */
void toplevel_set_mode(struct tinywl_toplevel *tl, int mode) {
	struct tinywl_server *server = tl->server;
	if (mode == tl->mode || !tl->star || !tl_ready(tl)) {
		return;
	}
	if (tl->mode == 0) {
		struct wlr_box geo = tl_geometry(tl), *g = &geo;
		tl->saved_wx = tl->wx;
		tl->saved_wy = tl->wy;
		tl->saved_w = g->width;
		tl->saved_h = g->height;
	}
	tl->mode = mode;
	if (mode == 0) {
		tl->wx = tl->saved_wx;
		tl->wy = tl->saved_wy;
		tl_set_size(tl, tl->saved_w, tl->saved_h);
	} else {
		struct wlr_box r = mode == 2 ? server->full : server->usable;
		int w = r.width ? r.width : STAR_W, h = r.height ? r.height : STAR_H;
		tl->wx = tl->star->cx - STAR_W / 2.0 + (r.width ? r.x - server->full.x : 0);
		tl->wy = tl->star->cy - STAR_H / 2.0 + (r.height ? r.y - server->full.y : 0);
		tl_set_size(tl, w, h);
	}
	tl_set_maximized(tl, mode == 1);
	tl_set_fullscreen(tl, mode == 2);
	layout_toplevel(tl);
	tile_star(server, tl->star);
	if (tl->star == server->current_star) {
		fly_to_star(server, tl->star);
	}
}

/* Window rules from `seiza-conf export rules`: tab-separated key=value lines. The first match wins. */
#define RULES_MAX 64
static struct rule {
	char app[64], title[64], star[32];
	int mode; /* 0 as it opens, 1 maximize */
	int w, h;
} g_rules[RULES_MAX];
static int g_nrules;

void rules_load(struct tinywl_server *server) {
	g_nrules = 0;
	char *kdl = cfg_kdl(server, "rules");
	for (char *save, *line = kdl ? strtok_r(kdl, "\n", &save) : NULL; line && g_nrules < RULES_MAX; line = strtok_r(NULL, "\n", &save)) {
		struct rule *r = &g_rules[g_nrules];
		memset(r, 0, sizeof(*r));
		for (char *s2, *f = strtok_r(line, "\t", &s2); f; f = strtok_r(NULL, "\t", &s2)) {
			if (!strncmp(f, "app=", 4)) snprintf(r->app, sizeof(r->app), "%s", f + 4);
			else if (!strncmp(f, "title=", 6)) snprintf(r->title, sizeof(r->title), "%s", f + 6);
			else if (!strncmp(f, "star=", 5)) snprintf(r->star, sizeof(r->star), "%s", f + 5);
			else if (!strcmp(f, "mode=maximize")) r->mode = 1;
			else if (!strncmp(f, "w=", 2)) r->w = atoi(f + 2);
			else if (!strncmp(f, "h=", 2)) r->h = atoi(f + 2);
		}
		g_nrules += r->app[0] || r->title[0];
	}
}

/* A window has just mapped: the first rule that fits sends it to its star, sizes it, maybe maximizes it.
 * A star name nobody has leaves it where it opened. */
void rules_apply(struct tinywl_toplevel *tl) {
	const char *app = tl->xsurface ? tl->xsurface->class : tl->xdg_toplevel ? tl->xdg_toplevel->app_id : NULL;
	const char *title = tl->xsurface ? tl->xsurface->title : tl->xdg_toplevel ? tl->xdg_toplevel->title : NULL;
	for (int i = 0; i < g_nrules; i++) {
		struct rule *r = &g_rules[i];
		if ((r->app[0] && (!app || strcasecmp(app, r->app))) || (r->title[0] && (!title || !strcasestr(title, r->title)))) {
			continue;
		}
		struct star *to = NULL, *s;
		wl_list_for_each(s, &tl->server->stars, link) {
			to = !to && r->star[0] && !strcmp(s->name, r->star) ? s : to;
		}
		if (to && to != tl->star) {
			int n = 0;
			struct tinywl_toplevel *o;
			wl_list_for_each(o, &tl->server->toplevels, link) {
				n += o->star == to;
			}
			tl->star = to;
			tl->wx = to->cx - STAR_W / 2.0 + 60 + 40 * (n % 8);
			tl->wy = to->cy - STAR_H / 2.0 + 60 + 40 * (n % 8);
		}
		if (r->w > 0 && r->h > 0) {
			tl_set_size(tl, r->w, r->h);
		}
		layout_toplevel(tl);
		if (r->mode == 1) {
			toplevel_set_mode(tl, 1);
		}
		return;
	}
}

/* Tiling: the first `max` windows of a star (oldest first) fill the area the bar leaves, a master on the left and the
 * others stacked on the right. Windows beyond max, and maximized ones, are left where they are.
 * shortcut: no scrolling strip for the windows beyond max yet. */
static struct { bool on; int max, gap; double master; } g_tile = {false, 3, 10, 0.55};

void tile_load(struct tinywl_server *server) {
	g_tile.on = false;
	g_tile.max = 3;
	g_tile.gap = 10;
	g_tile.master = 0.55;
	char *kdl = cfg_kdl(server, "layout");
	const char *k;
	if (kdl) {
		g_tile.on = (k = strstr(kdl, "tile=")) && k[5] == '1';
		if ((k = strstr(kdl, "max="))) g_tile.max = atoi(k + 4);
		if ((k = strstr(kdl, "gap="))) g_tile.gap = atoi(k + 4);
		if ((k = strstr(kdl, "master="))) g_tile.master = atof(k + 7);
	}
	g_tile.max = g_tile.max < 1 ? 1 : g_tile.max > 8 ? 8 : g_tile.max;
}

static bool star_tiled(const struct star *s) {
	return s->tiled ? s->tiled > 0 : g_tile.on;
}

void tile_star(struct tinywl_server *server, struct star *s) {
	if (!s || !star_tiled(s)) {
		return;
	}
	struct tinywl_toplevel *w[8], *tl;
	int n = 0;
	wl_list_for_each_reverse(tl, &server->all_toplevels, all_link) { /* the list has the newest first */
		struct wlr_surface *surf = tl_surface(tl);
		if (tl->star == s && tl->mode == 0 && surf && surf->mapped && tl_ready(tl) && n < g_tile.max) {
			w[n++] = tl;
		}
	}
	if (!n) {
		return;
	}
	struct wlr_box r = server->usable;
	int gap = g_tile.gap;
	double ax = s->cx - STAR_W / 2.0 + (r.width ? r.x - server->full.x : 0) + gap;
	double ay = s->cy - STAR_H / 2.0 + (r.height ? r.y - server->full.y : 0) + gap;
	int aw = (r.width ? r.width : STAR_W) - 2 * gap, ah = (r.height ? r.height : STAR_H) - 2 * gap;
	int mw = n == 1 ? aw : (int)((aw - gap) * g_tile.master);
	for (int i = 0; i < n; i++) {
		int x = (int)ax, y = (int)ay, tw = mw, th = ah;
		if (i > 0) { /* the stack: equal rows to the right of the master */
			int rows = n - 1;
			th = (ah - (rows - 1) * gap) / rows;
			x = (int)ax + mw + gap;
			y = (int)ay + (i - 1) * (th + gap);
			tw = aw - mw - gap;
		}
		tl_set_size(w[i], tw, th);
		struct wlr_box g = tl_geometry(w[i]);
		w[i]->wx = x - g.x;
		w[i]->wy = y - g.y;
		layout_toplevel(w[i]);
	}
}

static void op_toggle_tile(void *ud) {
	struct tinywl_server *server = ud;
	struct star *s = server->current_star;
	if (s) {
		s->tiled = star_tiled(s) ? -1 : 1;
		tile_star(server, s);
		persist_save(server);
	}
}

static void op_toggle_maximize(void *ud) {
	struct tinywl_toplevel *tl = focused_toplevel(ud);
	if (tl) {
		toplevel_set_mode(tl, tl->mode ? 0 : 1); /* also the way out of a fullscreen that has no key of its own */
	}
}

static void op_move_window(void *ud, int dx, int dy) {
	struct tinywl_toplevel *tl = focused_toplevel(ud);
	if (tl) {
		tl->wx += dx;
		tl->wy += dy;
		layout_toplevel(tl);
	}
}

static void op_resize_window(void *ud, int dw, int dh) {
	struct tinywl_toplevel *tl = focused_toplevel(ud);
	if (tl) {
		struct wlr_box g = tl_geometry(tl);
		tl_set_size(tl, fmax(MIN_WIN_W, g.width + dw), fmax(MIN_WIN_H, g.height + dh));
	}
}

static void op_close_window(void *ud) {
	struct tinywl_toplevel *tl = focused_toplevel(ud);
	if (tl) {
		tl_close(tl);
	}
}

/* Focus the nearest other window whose center lies in direction (dx, dy);
 * off-axis distance counts double so "right" prefers windows in the same row. */
static void op_focus_dir(void *ud, int dx, int dy) {
	struct tinywl_server *server = ud;
	struct tinywl_toplevel *cur = focused_toplevel(server), *tl, *best = NULL;
	if (!cur) {
		return;
	}
	double best_d = 1e18;
	wl_list_for_each(tl, &server->toplevels, link) {
		if (tl == cur) {
			continue;
		}
		double ox = tl->wx - cur->wx, oy = tl->wy - cur->wy;
		double along = ox * dx + oy * dy, off = fabs(ox * dy) + fabs(oy * dx);
		if (along > 0 && along + 2 * off < best_d) {
			best_d = along + 2 * off;
			best = tl;
		}
	}
	focus_toplevel(best);
}

static int op_goto_star(void *ud, int n) {
	struct tinywl_server *server = ud;
	struct star *s;
	wl_list_for_each(s, &server->stars, link) {
		if (--n == 0) {
			fly_to_star(server, s);
			return 0;
		}
	}
	return -1;
}

/* The help is a satellite; it reads the bindings keys_dump() wrote. */
static void op_help(void *ud) {
	op_spawn(ud, "seiza-help");
}

static int star_number(struct tinywl_server *server, const struct star *want) {
	int n = 1;
	struct star *s;
	wl_list_for_each(s, &server->stars, link) {
		if (s == want) {
			return n;
		}
		n++;
	}
	return 0;
}

/* $XDG_RUNTIME_DIR/seiza-windows, one window a line, most recently used first: star number, app id, title (tabs, newlines gone). */
static void op_switcher(void *ud) {
	struct tinywl_server *server = ud;
	const char *dir = getenv("XDG_RUNTIME_DIR");
	char path[300];
	snprintf(path, sizeof(path), "%s/seiza-windows", dir ? dir : "/tmp");
	FILE *f = fopen(path, "w");
	if (!f) {
		return;
	}
	struct tinywl_toplevel *tl;
	wl_list_for_each(tl, &server->toplevels, link) {
		const char *app = tl->xsurface ? tl->xsurface->class : tl->xdg_toplevel ? tl->xdg_toplevel->app_id : NULL;
		const char *title = tl->xsurface ? tl->xsurface->title : tl->xdg_toplevel ? tl->xdg_toplevel->title : NULL;
		char line[512];
		snprintf(line, sizeof(line), "%d\t%s\t%s", star_number(server, tl->star), app ? app : "", title ? title : "");
		for (char *c = line; *c; c++) {
			if (*c == '\n' || *c == '\r') {
				*c = ' ';
			}
		}
		fprintf(f, "%s\n", line);
	}
	fclose(f);
	op_spawn(server, "seiza-launcher --windows");
}

static int op_focus_window(void *ud, int n) {
	struct tinywl_server *server = ud;
	struct tinywl_toplevel *tl;
	wl_list_for_each(tl, &server->toplevels, link) {
		if (--n == 0) {
			if (tl->star && tl->star != server->current_star) {
				fly_to_star(server, tl->star);
			}
			focus_toplevel(tl);
			return 0;
		}
	}
	return -1;
}

/* Give a keyboard the xkb layout `layout` (e.g. "pt", "br", "us"; other rules come from XKB_DEFAULT_*). -1 and no change if xkb does not know it. */
int keyboard_set_layout(struct wlr_keyboard *kb, const char *layout) {
	struct xkb_rule_names names = {.layout = layout};
	struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
	struct xkb_keymap *keymap = xkb_keymap_new_from_names(context, &names, XKB_KEYMAP_COMPILE_NO_FLAGS);
	if (keymap) {
		wlr_keyboard_set_keymap(kb, keymap);
		xkb_keymap_unref(keymap);
	}
	xkb_context_unref(context);
	return keymap ? 0 : -1;
}

static int op_keymap(void *ud, const char *layout) {
	struct tinywl_server *server = ud;
	struct tinywl_keyboard *k;
	int r = 0;
	wl_list_for_each(k, &server->keyboards, link) {
		r |= keyboard_set_layout(k->wlr_keyboard, layout);
	}
	return r ? -1 : 0;
}

static int op_night(void *ud, const char *mode) {
	if (!strcmp(mode, "on")) {
		g_night_force = 1;
	} else if (!strcmp(mode, "off")) {
		g_night_force = 0;
	} else if (!strcmp(mode, "auto")) {
		g_night_force = -1;
	} else {
		return -1;
	}
	night_apply(ud);
	return 0;
}

const struct cmd_ops seiza_ops = {
	op_overview, op_new_star, op_delete_star, op_save, op_load, op_new_constellation, op_relaunch,
	op_reload_config, op_quit, op_pan, op_zoom_reset, op_cycle_window, op_spawn,
	op_move_window, op_resize_window, op_close_window, op_goto_star, op_help, op_focus_dir,
	op_toggle_maximize, op_set_figure, op_rename_star, op_add_star_at, op_move_star, op_debug_windows, op_debug_pointer, op_toggle_link,
	op_switcher, op_focus_window, op_toggle_tile, op_night, op_keymap,
};


/* Sends stderr (ours and every program we spawn) to <state dir>/seiza/seiza.log, keeping the
 * previous run as seiza.log.old. SEIZA_LOG=stderr opts out. Failure leaves stderr untouched. */
static void log_to_file(void) {
	const char *opt = getenv("SEIZA_LOG");
	if (opt && !strcmp(opt, "stderr")) {
		return;
	}
	const char *state = getenv("XDG_STATE_HOME"), *home = getenv("HOME");
	char dir[512], path[600], old[610];
	if (state && state[0]) {
		snprintf(dir, sizeof dir, "%s/seiza", state);
	} else if (home && home[0]) {
		snprintf(dir, sizeof dir, "%s/.local/state/seiza", home);
	} else {
		return;
	}
	char parent[512];
	snprintf(parent, sizeof parent, "%s", dir);
	char *slash = strrchr(parent, '/');
	if (slash) {
		*slash = 0;
		mkdir(parent, 0755); /* shortcut: only the last two levels are created */
	}
	mkdir(dir, 0755);
	snprintf(path, sizeof path, "%s/seiza.log", dir);
	snprintf(old, sizeof old, "%s.old", path);
	rename(path, old);
	if (freopen(path, "w", stderr)) {
		setvbuf(stderr, NULL, _IOLBF, 0);
	}
}

int main(int argc, char *argv[]) {
	/* `seiza validate` checks ~/.config/seiza/seiza.kdl; it needs no running compositor. */
	if (argc == 2 && !strcmp(argv[1], "validate")) {
		execlp("seiza-conf", "seiza-conf", "check", (char *)NULL);
		perror("seiza-conf (is it installed? make install)");
		return 127;
	}
	/* `seiza <command>` from a shell talks to the running compositor. */
	if (argc > 1 && argv[1][0] != '-') {
		return ipc_client(argc, argv);
	}
	log_to_file();
	wlr_log_init(WLR_DEBUG, NULL);
	char *startup_cmd = NULL;

	int c;
	while ((c = getopt(argc, argv, "s:h")) != -1) {
		switch (c) {
		case 's':
			startup_cmd = optarg;
			break;
		default:
			printf("Usage: %s [-s startup command]\n", argv[0]);
			return 0;
		}
	}
	if (optind < argc) {
		printf("Usage: %s [-s startup command]\n", argv[0]);
		return 0;
	}

	struct tinywl_server server = {0};
	/* The Wayland display is managed by libwayland. It handles accepting
	 * clients from the Unix socket, managing Wayland globals, and so on. */
	server.wl_display = wl_display_create();
	/* The backend is a wlroots feature which abstracts the underlying input and
	 * output hardware. The autocreate option will choose the most suitable
	 * backend based on the current environment, such as opening an X11 window
	 * if an X11 server is running. */
	server.backend = wlr_backend_autocreate(wl_display_get_event_loop(server.wl_display), NULL);
	if (server.backend == NULL) {
		wlr_log(WLR_ERROR, "failed to create wlr_backend");
		return 1;
	}

	/* Autocreates a renderer, either Pixman, GLES2 or Vulkan for us. The user
	 * can also specify a renderer using the WLR_RENDERER env var.
	 * The renderer is responsible for defining the various pixel formats it
	 * supports for shared memory, this configures that for clients. */
	server.renderer = fx_renderer_create(server.backend);
	if (server.renderer == NULL) {
		wlr_log(WLR_ERROR, "failed to create wlr_renderer");
		return 1;
	}

	wlr_renderer_init_wl_display(server.renderer, server.wl_display);

	/* Autocreates an allocator for us.
	 * The allocator is the bridge between the renderer and the backend. It
	 * handles the buffer creation, allowing wlroots to render onto the
	 * screen */
	server.allocator = wlr_allocator_autocreate(server.backend,
		server.renderer);
	if (server.allocator == NULL) {
		wlr_log(WLR_ERROR, "failed to create wlr_allocator");
		return 1;
	}

	/* This creates some hands-off wlroots interfaces. The compositor is
	 * necessary for clients to allocate surfaces, the subcompositor allows to
	 * assign the role of subsurfaces to surfaces and the data device manager
	 * handles the clipboard. Each of these wlroots interfaces has room for you
	 * to dig your fingers in and play with their behavior if you want. Note that
	 * the clients cannot set the selection directly without compositor approval,
	 * see the handling of the request_set_selection event below.*/
	struct wlr_compositor *compositor = wlr_compositor_create(server.wl_display, 5, server.renderer);
	wlr_subcompositor_create(server.wl_display);
	wlr_data_device_manager_create(server.wl_display);
	wlr_data_control_manager_v1_create(server.wl_display); /* SEIZA: lets wl-paste --watch see the clipboard (make clipboard) */
	/* SEIZA: screenshots (grim) and screen sharing (portals) */
	wlr_screencopy_manager_v1_create(server.wl_display);
	server.relative_pointer = wlr_relative_pointer_manager_v1_create(server.wl_display);
	server.constraints = wlr_pointer_constraints_v1_create(server.wl_display);
	server.new_constraint.notify = server_new_constraint;
	wl_signal_add(&server.constraints->events.new_constraint, &server.new_constraint);
	wlr_ext_output_image_capture_source_manager_v1_create(server.wl_display, 1);
	wlr_ext_image_copy_capture_manager_v1_create(server.wl_display, 1);

	/* Creates an output layout, which a wlroots utility for working with an
	 * arrangement of screens in a physical layout. */
	server.output_layout = wlr_output_layout_create(server.wl_display);

	/* Configure a listener to be notified when new outputs are available on the
	 * backend. */
	wl_list_init(&server.outputs);
	server.new_output.notify = server_new_output;
	wl_signal_add(&server.backend->events.new_output, &server.new_output);

	/* Create a scene graph. This is a wlroots abstraction that handles all
	 * rendering and damage tracking. All the compositor author needs to do
	 * is add things that should be rendered to the scene graph at the proper
	 * positions and then call wlr_scene_output_commit() to render a frame if
	 * necessary.
	 */
	server.scene = wlr_scene_create();
	server.scene_layout = wlr_scene_attach_output_layout(server.scene, server.output_layout);
	server.layers[0] = wlr_scene_tree_create(&server.scene->tree);
	server.layers[1] = wlr_scene_tree_create(&server.scene->tree);
	server.canvas = wlr_scene_tree_create(&server.scene->tree);
	server.xtop = wlr_scene_tree_create(server.canvas);
	wl_list_init(&server.unmanaged);
	server.layers[2] = wlr_scene_tree_create(&server.scene->tree);
	server.layers[3] = wlr_scene_tree_create(&server.scene->tree);
	server.lock_tree = wlr_scene_tree_create(&server.scene->tree);
	server.corner_tree = wlr_scene_tree_create(&server.scene->tree);
	server.idle_inhibit = wlr_idle_inhibit_v1_create(server.wl_display);
	server.dim_rect = wlr_scene_rect_create(server.corner_tree, 1, 1, (float[4]){0, 0, 0, 0.6f});
	wlr_scene_node_set_enabled(&server.dim_rect->node, false);
	wl_list_init(&server.layer_surfaces);
	server.zoom = 1.0;
	wl_list_init(&server.stars);
	wl_list_init(&server.ghosts);
	/* Start in star 0: its territory is centered so the default camera
	 * (0,0) already looks at it on a 1920x1080 screen. */
	persist_startup(&server);
	keys_load(&server);
	rules_load(&server);
	tile_load(&server);
	g_last_input_ms = now_ms();
	g_idle_timer = wl_event_loop_add_timer(wl_display_get_event_loop(server.wl_display), idle_tick, &server);
	g_night_timer = wl_event_loop_add_timer(wl_display_get_event_loop(server.wl_display), night_tick, &server);
	wl_event_source_timer_update(g_night_timer, 1000);
	frame_colors_load(&server);

	/* Set up xdg-shell version 3. The xdg-shell is a Wayland protocol which is
	 * used for application windows. For more detail on shells, refer to
	 * https://drewdevault.com/2018/07/29/Wayland-shells.html.
	 */
	wl_list_init(&server.toplevels);
	wl_list_init(&server.all_toplevels);
	fly_to_star(&server, server.current_star); /* restored stars may not sit at the origin */
	server.xdg_shell = wlr_xdg_shell_create(server.wl_display, 3);
	/* XWayland, started when the first X11 program connects. The programs we launch inherit
	 * DISPLAY from here, so setting it is all they need to find it. */
	/* Read before XWayland sets DISPLAY below, or every session would look nested. */
	int real_session = !getenv("WAYLAND_DISPLAY") && !getenv("DISPLAY");
	server.xwayland = wlr_xwayland_create(server.wl_display, compositor, true);
	if (server.xwayland) {
		server.new_xwayland_surface.notify = server_new_xwayland_surface;
		wl_signal_add(&server.xwayland->events.new_surface, &server.new_xwayland_surface);
		server.xwayland_ready.notify = xwayland_ready;
		wl_signal_add(&server.xwayland->events.ready, &server.xwayland_ready);
		setenv("DISPLAY", server.xwayland->display_name, true);
	} else {
		wlr_log(WLR_ERROR, "seiza: could not set up XWayland: X11 programs will not run");
	}

	struct wlr_xdg_decoration_manager_v1 *deco = wlr_xdg_decoration_manager_v1_create(server.wl_display);
	static struct wl_listener new_deco = {.notify = server_new_decoration};
	wl_signal_add(&deco->events.new_toplevel_decoration, &new_deco);
	server.new_xdg_toplevel.notify = server_new_xdg_toplevel;
	wl_signal_add(&server.xdg_shell->events.new_toplevel, &server.new_xdg_toplevel);
	wlr_viewporter_create(server.wl_display);
	wlr_xdg_output_manager_v1_create(server.wl_display, server.output_layout);
	server.lock_mgr = wlr_session_lock_manager_v1_create(server.wl_display);
	server.new_lock.notify = server_new_lock;
	wl_signal_add(&server.lock_mgr->events.new_lock, &server.new_lock);
	server.layer_shell = wlr_layer_shell_v1_create(server.wl_display, 4);
	server.new_layer_surface.notify = server_new_layer_surface;
	wl_signal_add(&server.layer_shell->events.new_surface, &server.new_layer_surface);
	server.new_xdg_popup.notify = server_new_xdg_popup;
	wl_signal_add(&server.xdg_shell->events.new_popup, &server.new_xdg_popup);

	/*
	 * Creates a cursor, which is a wlroots utility for tracking the cursor
	 * image shown on screen.
	 */
	server.cursor = wlr_cursor_create();
	wlr_cursor_attach_output_layout(server.cursor, server.output_layout);

	/* Creates an xcursor manager, another wlroots utility which loads up
	 * Xcursor themes to source cursor images from and makes sure that cursor
	 * images are available at all scale factors on the screen (necessary for
	 * HiDPI support). */
	server.cursor_mgr = wlr_xcursor_manager_create(NULL, 24);

	/*
	 * wlr_cursor *only* displays an image on screen. It does not move around
	 * when the pointer moves. However, we can attach input devices to it, and
	 * it will generate aggregate events for all of them. In these events, we
	 * can choose how we want to process them, forwarding them to clients and
	 * moving the cursor around. More detail on this process is described in
	 * https://drewdevault.com/2018/07/17/Input-handling-in-wlroots.html.
	 *
	 * And more comments are sprinkled throughout the notify functions above.
	 */
	server.cursor_mode = TINYWL_CURSOR_PASSTHROUGH;
	server.cursor_motion.notify = server_cursor_motion;
	wl_signal_add(&server.cursor->events.motion, &server.cursor_motion);
	server.cursor_motion_absolute.notify = server_cursor_motion_absolute;
	wl_signal_add(&server.cursor->events.motion_absolute,
			&server.cursor_motion_absolute);
	server.cursor_button.notify = server_cursor_button;
	wl_signal_add(&server.cursor->events.button, &server.cursor_button);
	server.cursor_axis.notify = server_cursor_axis;
	wl_signal_add(&server.cursor->events.axis, &server.cursor_axis);
	server.cursor_frame.notify = server_cursor_frame;
	wl_signal_add(&server.cursor->events.frame, &server.cursor_frame);

	/*
	 * Configures a seat, which is a single "seat" at which a user sits and
	 * operates the computer. This conceptually includes up to one keyboard,
	 * pointer, touch, and drawing tablet device. We also rig up a listener to
	 * let us know when new input devices are available on the backend.
	 */
	wl_list_init(&server.keyboards);
	server.new_input.notify = server_new_input;
	wl_signal_add(&server.backend->events.new_input, &server.new_input);
	server.seat = wlr_seat_create(server.wl_display, "seat0");
	server.request_cursor.notify = seat_request_cursor;
	wl_signal_add(&server.seat->events.request_set_cursor,
			&server.request_cursor);
	server.pointer_focus_change.notify = seat_pointer_focus_change;
	wl_signal_add(&server.seat->pointer_state.events.focus_change,
			&server.pointer_focus_change);
	server.request_set_selection.notify = seat_request_set_selection;
	wl_signal_add(&server.seat->events.request_set_selection,
			&server.request_set_selection);

	/* Add a Unix socket to the Wayland display. */
	const char *socket = wl_display_add_socket_auto(server.wl_display);
	if (!socket) {
		wlr_backend_destroy(server.backend);
		return 1;
	}

	/* Start the backend. This will enumerate outputs and inputs, become the DRM
	 * master, etc */
	if (!wlr_backend_start(server.backend)) {
		wlr_backend_destroy(server.backend);
		wl_display_destroy(server.wl_display);
		return 1;
	}

	/* Set the WAYLAND_DISPLAY environment variable to our socket and run the
	 * startup command if requested. */
	setenv("WAYLAND_DISPLAY", socket, true);
	/* SEIZA: portals pick their backends by desktop name (portals/seiza-portals.conf).
	 * Only a real session publishes this to dbus/systemd: a nested one must not repoint the host's portals. */
	setenv("XDG_CURRENT_DESKTOP", "seiza", true);
	setenv("XDG_SESSION_TYPE", "wayland", true);
	g_env_ready = true;
	struct tinywl_output *mon;
	wl_list_for_each(mon, &server.outputs, link) { /* monitors that were there from the start */
		start_starfield(&server, mon);
	}
	if (real_session && fork() == 0) {
		execlp("dbus-update-activation-environment", "dbus-update-activation-environment", "--systemd",
			"WAYLAND_DISPLAY", "XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE", (void *)NULL);
		_exit(1);
	}
	if (startup_cmd) {
		if (fork() == 0) {
			execl("/bin/sh", "/bin/sh", "-c", startup_cmd, (void *)NULL);
		}
	}
	/* Run the Wayland event loop. This does not return until you exit the
	 * compositor. Starting the backend rigged up all of the necessary event
	 * loop configuration to listen to libinput events, DRM events, generate
	 * frame events at the refresh rate, and so on. */
	wlr_log(WLR_INFO, "Running Wayland compositor on WAYLAND_DISPLAY=%s",
			socket);
	wl_event_loop_add_signal(wl_display_get_event_loop(server.wl_display), SIGCHLD, reap_children, NULL);
	ipc_serve(wl_display_get_event_loop(server.wl_display), &seiza_ops, &server);
	autostart(&server);
	watch_config(&server);
	wl_display_run(server.wl_display);
	ipc_close();


	/* Once wl_display_run returns, we destroy all clients then shut down the
	 * server. */
	if (server.xwayland) {
		wl_list_remove(&server.new_xwayland_surface.link);
		wl_list_remove(&server.xwayland_ready.link);
		wlr_xwayland_destroy(server.xwayland);
	}
	wl_display_destroy_clients(server.wl_display);

	wl_list_remove(&server.new_xdg_toplevel.link);
	wl_list_remove(&server.new_xdg_popup.link);
	wl_list_remove(&server.new_layer_surface.link);
	wl_list_remove(&server.new_lock.link);
	wl_list_remove(&new_deco.link); /* wlroots asserts that no listener is left on the manager when the display goes */
	wl_list_remove(&server.new_constraint.link);
	if (server.constraint) {
		wl_list_remove(&server.constraint_destroy.link);
	}

	wl_list_remove(&server.cursor_motion.link);
	wl_list_remove(&server.cursor_motion_absolute.link);
	wl_list_remove(&server.cursor_button.link);
	wl_list_remove(&server.cursor_axis.link);
	wl_list_remove(&server.cursor_frame.link);

	wl_list_remove(&server.new_input.link);
	wl_list_remove(&server.request_cursor.link);
	wl_list_remove(&server.pointer_focus_change.link);
	wl_list_remove(&server.request_set_selection.link);

	wl_list_remove(&server.new_output.link);

	/* The corner masks hang off the scene; output_destroy (run by the backend below) must not free them twice. */
	struct tinywl_output *out;
	wl_list_for_each(out, &server.outputs, link) {
		for (int i = 0; i < 4; i++) {
			if (out->corner[i]) {
				wlr_scene_node_destroy(&out->corner[i]->node);
				out->corner[i] = NULL;
			}
		}
	}
	wlr_scene_node_destroy(&server.scene->tree.node);
	wlr_xcursor_manager_destroy(server.cursor_mgr);
	wlr_cursor_destroy(server.cursor);
	wlr_allocator_destroy(server.allocator);
	wlr_renderer_destroy(server.renderer);
	wlr_backend_destroy(server.backend);
	wl_display_destroy(server.wl_display);
	return 0;
}
