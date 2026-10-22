/* SEIZA: promoted from spike/ on 2026-10-22 (design: docs/2026-10-22-constellations-design.md) */
#include <assert.h>
#include <getopt.h>
#include <stdbool.h>
#include <math.h>
#include <stdlib.h>
#include <stdio.h>
#include <time.h>
#include <unistd.h>
#include <wayland-server-core.h>
#include <wlr/backend.h>
#include <wlr/render/allocator.h>
#include <wlr/render/wlr_renderer.h>
#include <wlr/types/wlr_cursor.h>
#include <wlr/types/wlr_compositor.h>
#include <wlr/types/wlr_data_device.h>
#include <wlr/types/wlr_input_device.h>
#include <linux/input-event-codes.h>
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_output_layout.h>
#include <wlr/types/wlr_pointer.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_subcompositor.h>
#include <wlr/types/wlr_xcursor_manager.h>
#include <wlr/types/wlr_xdg_shell.h>
#include <wlr/util/log.h>
#include <xkbcommon/xkbcommon.h>
#include "cmd.h"
#include "constellation.h"
#include "ipc.h"
#include "procinfo.h"
#include "keys.h"

/* For brevity's sake, struct members are annotated where they are used. */
enum tinywl_cursor_mode {
	TINYWL_CURSOR_PASSTHROUGH,
	TINYWL_CURSOR_MOVE,
	TINYWL_CURSOR_RESIZE,
	TINYWL_CURSOR_PAN, /* SEIZA: dragging the canvas */
};

/* SEIZA: a star is a workspace: a named spot on the canvas that owns windows.
 * Its `marker` (a dot plus a frame around its territory) only shows in the
 * overview. All positions here are in world coordinates. */
struct star {
	struct wl_list link;
	char name[32];
	double cx, cy; /* center of the star's territory */
	struct wlr_scene_tree *marker;
	struct wlr_scene_rect *frame[4];
	struct wlr_scene_rect *dot;
	float color[4];
	char apps[CSTAR_APPS_MAX][256]; /* remembered launch commands (never auto-launched) */
	int napps;
};

struct tinywl_server {
	struct wl_display *wl_display;
	struct wlr_backend *backend;
	struct wlr_renderer *renderer;
	struct wlr_allocator *allocator;
	struct wlr_scene *scene;
	struct wlr_scene_output_layout *scene_layout;

	/* SEIZA: every window lives under `canvas`. Windows have a position in
	 * WORLD coordinates (tl->wx, tl->wy). What is on screen is
	 *     screen = world * zoom - cam
	 * so cam is in screen pixels at the current zoom, and the canvas node's
	 * offset is -cam. */
	struct wlr_scene_tree *canvas;
	double cam_x, cam_y;
	double zoom;

	double pan_last_x, pan_last_y; /* cursor position at the last canvas-drag step */
	struct wl_list stars;
	struct star *current_star; /* the star new windows land in */
	char cfg_dir[400];         /* ~/.config/seiza */
	struct keymap_cfg keys;    /* built-in defaults + ~/.config/seiza/keys */
	char constellation_name[32]; /* the active constellation (autosaved) */
	bool overview;             /* zoomed-out constellation view */
	struct star *hover_star;   /* star under the cursor in the overview */
	struct wl_event_source *hover_timer; /* dwell before flying into it */

	struct wlr_xdg_shell *xdg_shell;
	struct wl_listener new_xdg_toplevel;
	struct wl_listener new_xdg_popup;
	struct wl_list toplevels;

	struct wlr_cursor *cursor;
	struct wlr_xcursor_manager *cursor_mgr;
	struct wl_listener cursor_motion;
	struct wl_listener cursor_motion_absolute;
	struct wl_listener cursor_button;
	struct wl_listener cursor_axis;
	struct wl_listener cursor_frame;

	struct wlr_seat *seat;
	struct wl_listener new_input;
	struct wl_listener request_cursor;
	struct wl_listener pointer_focus_change;
	struct wl_listener request_set_selection;
	struct wl_list keyboards;
	enum tinywl_cursor_mode cursor_mode;
	struct tinywl_toplevel *grabbed_toplevel;
	double grab_x, grab_y;
	struct wlr_box grab_geobox;
	uint32_t resize_edges;

	struct wlr_output_layout *output_layout;
	struct wl_list outputs;
	struct wl_listener new_output;
};

struct tinywl_output {
	struct wl_list link;
	struct tinywl_server *server;
	struct wlr_output *wlr_output;
	struct wl_listener frame;
	struct wl_listener request_state;
	struct wl_listener destroy;
};

struct tinywl_toplevel {
	struct wl_list link;
	struct tinywl_server *server;
	struct wlr_xdg_toplevel *xdg_toplevel;
	struct wlr_scene_tree *scene_tree;
	double wx, wy; /* SEIZA: window origin in world coordinates */
	struct star *star; /* SEIZA: the workspace this window belongs to */
	/* SEIZA debug frame: top, bottom, left, right strips around the window. */
	struct wlr_scene_rect *border[4];
	struct wl_listener map;
	struct wl_listener unmap;
	struct wl_listener commit;
	struct wl_listener destroy;
	struct wl_listener request_move;
	struct wl_listener request_resize;
	struct wl_listener request_maximize;
	struct wl_listener request_fullscreen;
};

struct tinywl_popup {
	struct wlr_xdg_popup *xdg_popup;
	struct wl_listener commit;
	struct wl_listener destroy;
};

struct tinywl_keyboard {
	struct wl_list link;
	struct tinywl_server *server;
	struct wlr_keyboard *wlr_keyboard;

	struct wl_listener modifiers;
	struct wl_listener key;
	struct wl_listener destroy;
};

/* SEIZA debug frame: thickness in px and colors (RGBA). */
#define BORDER_W 2
static const float BORDER_FOCUSED[4] = {0.95f, 0.60f, 0.90f, 1.0f};
static const float BORDER_IDLE[4]    = {0.40f, 0.40f, 0.50f, 1.0f};

/* Fit the frame around the window's current geometry and color it by focus.
 * The strips sit just outside the geometry so they never cover the app. */
static void update_border(struct tinywl_toplevel *tl) {
	struct wlr_box *g = &tl->xdg_toplevel->base->geometry;
	bool focused = tl->server->seat->keyboard_state.focused_surface ==
		tl->xdg_toplevel->base->surface;
	const float *c = focused ? BORDER_FOCUSED : BORDER_IDLE;
	int t = BORDER_W; /* stays constant on screen, whatever the zoom */
	double z = tl->server->zoom;
	int gx = lround(g->x * z), gy = lround(g->y * z);
	int gw = lround(g->width * z), gh = lround(g->height * z);
	struct { int x, y, w, h; } r[4] = {
		{gx - t, gy - t,  gw + 2 * t, t},  /* top */
		{gx - t, gy + gh, gw + 2 * t, t},  /* bottom */
		{gx - t, gy,      t,          gh}, /* left */
		{gx + gw, gy,     t,          gh}, /* right */
	};
	for (int i = 0; i < 4; i++) {
		wlr_scene_rect_set_size(tl->border[i], r[i].w, r[i].h);
		wlr_scene_node_set_position(&tl->border[i]->node, r[i].x, r[i].y);
		wlr_scene_rect_set_color(tl->border[i], c);
	}
}

/* SEIZA stars. Territory size and grid spacing, in world units. New stars are
 * placed on a 3-column grid, left to right, then down. */
#define STAR_W 1920
#define STAR_H 1080
#define STAR_DX 2400
#define STAR_DY 1500
#define STAR_DOT 28    /* screen px, constant at any zoom */
#define STAR_LINE 3    /* screen px */
#define HOVER_MS 450   /* dwell over a star in the overview before flying in */

static void screen_center(struct tinywl_server *server, double *x, double *y) {
	struct wlr_box b = {0};
	wlr_output_layout_get_box(server->output_layout, NULL, &b);
	*x = (b.width > 0 ? b.width : 1920) / 2.0;
	*y = (b.height > 0 ? b.height : 1080) / 2.0;
}

/* Place a star's marker for the current camera/zoom; brighten it if hovered. */
static void layout_star(struct tinywl_server *server, struct star *s) {
	double z = server->zoom;
	wlr_scene_node_set_enabled(&s->marker->node, server->overview);
	int cx = lround(s->cx * z), cy = lround(s->cy * z);
	int hw = lround(STAR_W / 2.0 * z), hh = lround(STAR_H / 2.0 * z);
	int t = STAR_LINE;
	float c[4] = {s->color[0], s->color[1], s->color[2], 1.0f};
	if (server->hover_star == s) {
		c[0] = c[1] = c[2] = 1.0f;
	}
	struct { int x, y, w, h; } r[4] = {
		{cx - hw - t, cy - hh - t, 2 * hw + 2 * t, t},  /* top */
		{cx - hw - t, cy + hh,     2 * hw + 2 * t, t},  /* bottom */
		{cx - hw - t, cy - hh,     t,              2 * hh}, /* left */
		{cx + hw,     cy - hh,     t,              2 * hh}, /* right */
	};
	for (int i = 0; i < 4; i++) {
		wlr_scene_rect_set_size(s->frame[i], r[i].w, r[i].h);
		wlr_scene_node_set_position(&s->frame[i]->node, r[i].x, r[i].y);
		wlr_scene_rect_set_color(s->frame[i], c);
	}
	wlr_scene_rect_set_size(s->dot, STAR_DOT, STAR_DOT);
	wlr_scene_node_set_position(&s->dot->node, cx - STAR_DOT / 2, cy - STAR_DOT / 2);
	wlr_scene_rect_set_color(s->dot, c);
}

/* Make a star at a world position. Its color follows its creation order. */
static struct star *star_create_at(struct tinywl_server *server, const char *name,
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

static struct star *star_at(struct tinywl_server *server, double wx, double wy);

/* Make a new star in the first free slot of the 3-column grid. */
static struct star *star_create(struct tinywl_server *server) {
	int slot = 0;
	double cx, cy;
	for (;; slot++) {
		cx = STAR_W / 2.0 + (slot % 3) * STAR_DX;
		cy = STAR_H / 2.0 + (slot / 3) * STAR_DY;
		if (!star_at(server, cx, cy)) {
			break;
		}
	}
	char name[32];
	snprintf(name, sizeof(name), "star %d", slot);
	return star_create_at(server, name, cx, cy);
}

/* Which star's territory contains this world point, if any. */
static struct star *star_at(struct tinywl_server *server, double wx, double wy) {
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
static int persist_write(struct tinywl_server *server, const char *name, bool make_last) {
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
		memcpy(cs->apps, s->apps, sizeof(cs->apps));
	}
	if (constellation_save(&c, server->cfg_dir) != 0 ||
			(make_last && constellation_set_last(server->cfg_dir, c.name) != 0)) {
		wlr_log(WLR_ERROR, "seiza: could not save constellation '%s' in %s",
			c.name, server->cfg_dir);
		return -1;
	}
	return 0;
}

static void persist_save(struct tinywl_server *server) {
	persist_write(server, server->constellation_name, true);
}

/* Build the stars of `c` and make the first one current. */
static void persist_apply(struct tinywl_server *server, const struct constellation *c) {
	snprintf(server->constellation_name, sizeof(server->constellation_name), "%s", c->name);
	for (int i = 0; i < c->nstars; i++) {
		struct star *s = star_create_at(server, c->stars[i].name, c->stars[i].x, c->stars[i].y);
		s->napps = c->stars[i].napps;
		memcpy(s->apps, c->stars[i].apps, sizeof(s->apps));
	}
	server->current_star = wl_container_of(server->stars.next, server->current_star, link);
}

/* Startup: open the constellation named in `last`, else the default one. */
static void persist_startup(struct tinywl_server *server) {
	struct constellation c;
	char name[32];
	constellation_config_dir(server->cfg_dir, sizeof(server->cfg_dir));
	if (constellation_get_last(server->cfg_dir, name, sizeof(name)) != 0 ||
			constellation_load(server->cfg_dir, name, &c) != 0) {
		wlr_log(WLR_INFO, "seiza: no usable saved constellation, starting 'default'");
		constellation_default(&c);
	}
	persist_apply(server, &c);
}

/* The current star follows the camera: whichever star's territory is under the
 * screen center. In empty space between stars it stays as it was. */
static void update_current_star(struct tinywl_server *server) {
	double sx, sy;
	screen_center(server, &sx, &sy);
	struct star *s = star_at(server, (sx + server->cam_x) / server->zoom,
		(sy + server->cam_y) / server->zoom);
	if (s && s != server->current_star) {
		server->current_star = s;
		wlr_log(WLR_INFO, "seiza: now in %s", s->name);
	}
}

/* SEIZA limits. Past ~2x the stretched pixels turn grainy, below ~0.25x
 * windows become unreadable specks. Window sizes are in world units. */
#define ZOOM_MIN 0.25
#define ZOOM_MAX 2.0
#define MIN_WIN_W 200
#define MIN_WIN_H 120

/* SEIZA zoom. wlroots has no "scale this subtree", so each surface buffer is
 * drawn at a scaled size. wlroots resets that size on every client commit, so
 * this is re-applied from xdg_toplevel_commit.
 * shortcut: subsurface/popup buffers get scaled but not their offsets, and a
 * nonzero xdg geometry offset (CSD shadows) is not scaled either; fix when a
 * real app needs it. */
static void scale_buffer(struct wlr_scene_buffer *sb, int sx, int sy, void *data) {
	double z = *(double *)data;
	struct wlr_scene_surface *ss = wlr_scene_surface_try_from_buffer(sb);
	if (!ss) {
		return;
	}
	/* The unscaled size wlroots itself uses (surface.c: surface_reconfigure);
	 * xdg windows aren't clipped, so it is just the surface size. */
	int w = ss->surface->current.width, h = ss->surface->current.height;
	if (w <= 0 || h <= 0) {
		return;
	}
	wlr_scene_buffer_set_dest_size(sb, fmax(1, lround(w * z)), fmax(1, lround(h * z)));
}

/* Put one window where world + camera + zoom say it should be. */
static void layout_toplevel(struct tinywl_toplevel *tl) {
	double z = tl->server->zoom;
	wlr_scene_node_set_position(&tl->scene_tree->node, lround(tl->wx * z), lround(tl->wy * z));
	wlr_scene_node_for_each_buffer(&tl->scene_tree->node, scale_buffer, &z);
	update_border(tl);
}

/* Re-place the canvas and every window; call after the camera or zoom changes. */
static void relayout_all(struct tinywl_server *server) {
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
}

/* Multiply the zoom by `factor`, keeping the world point under the cursor
 * fixed on screen (so it zooms toward the pointer, like a map). */
static void zoom_at(struct tinywl_server *server, double factor) {
	if (server->overview) {
		return; /* the overview has its own fixed fit; leave it with Alt+O */
	}
	double z = fmin(ZOOM_MAX, fmax(ZOOM_MIN, server->zoom * factor));
	factor = z / server->zoom;
	server->cam_x = (server->cursor->x + server->cam_x) * factor - server->cursor->x;
	server->cam_y = (server->cursor->y + server->cam_y) * factor - server->cursor->y;
	server->zoom = z;
	relayout_all(server);
}

/* Jump the camera into a star at 1:1 zoom (instant for now, no animation) and
 * make it the star new windows land in. */
static void fly_to_star(struct tinywl_server *server, struct star *s) {
	double sx, sy;
	screen_center(server, &sx, &sy);
	server->current_star = s;
	server->overview = false;
	server->hover_star = NULL;
	wl_event_source_timer_update(server->hover_timer, 0);
	server->zoom = 1.0;
	server->cam_x = s->cx - sx;
	server->cam_y = s->cy - sy;
	wlr_log(WLR_INFO, "seiza: entered %s", s->name);
	relayout_all(server);
}

/* Zoom out so every star fits on screen and show the star markers. This
 * ignores ZOOM_MIN on purpose: fitting the whole constellation needs more.
 * shortcut: fit uses star centers + territory, so it is wrong if the output
 * is a different aspect ratio than 16:9, fine for the spike. */
static void enter_overview(struct tinywl_server *server) {
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
	server->zoom = fmin(1.0, fmax(0.05, z));
	server->cam_x = (minx + maxx) / 2 * server->zoom - sx;
	server->cam_y = (miny + maxy) / 2 * server->zoom - sy;
	server->overview = true;
	/* Markers go above the (shrunk) windows so the stars stay visible. */
	wl_list_for_each(s, &server->stars, link) {
		wlr_scene_node_raise_to_top(&s->marker->node);
	}
	wlr_seat_pointer_clear_focus(server->seat);
	relayout_all(server);
}

/* The dwell timer fired: the cursor rested on a star in the overview. */
static int hover_timeout(void *data) {
	struct tinywl_server *server = data;
	if (server->overview && server->hover_star) {
		fly_to_star(server, server->hover_star);
	}
	return 0;
}

/* Overview pointer handling: track which star is hovered, restart the dwell. */
static void overview_pointer(struct tinywl_server *server) {
	struct star *s = star_at(server,
		(server->cursor->x + server->cam_x) / server->zoom,
		(server->cursor->y + server->cam_y) / server->zoom);
	if (s != server->hover_star) {
		server->hover_star = s;
		wl_event_source_timer_update(server->hover_timer, s ? HOVER_MS : 0);
		relayout_all(server); /* recolors the hovered star */
	}
}

static bool alt_held(struct tinywl_server *server) {
	struct wlr_keyboard *kb = wlr_seat_get_keyboard(server->seat);
	return kb && (wlr_keyboard_get_modifiers(kb) & WLR_MODIFIER_ALT);
}

static void focus_toplevel(struct tinywl_toplevel *toplevel) {
	/* Note: this function only deals with keyboard focus. */
	if (toplevel == NULL) {
		return;
	}
	struct tinywl_server *server = toplevel->server;
	struct wlr_seat *seat = server->seat;
	struct wlr_surface *prev_surface = seat->keyboard_state.focused_surface;
	struct wlr_surface *surface = toplevel->xdg_toplevel->base->surface;
	if (prev_surface == surface) {
		/* Don't re-focus an already focused surface. */
		return;
	}
	if (prev_surface) {
		/*
		 * Deactivate the previously focused surface. This lets the client know
		 * it no longer has focus and the client will repaint accordingly, e.g.
		 * stop displaying a caret.
		 */
		struct wlr_xdg_toplevel *prev_toplevel =
			wlr_xdg_toplevel_try_from_wlr_surface(prev_surface);
		if (prev_toplevel != NULL) {
			wlr_xdg_toplevel_set_activated(prev_toplevel, false);
		}
	}
	struct wlr_keyboard *keyboard = wlr_seat_get_keyboard(seat);
	/* Move the toplevel to the front */
	wlr_scene_node_raise_to_top(&toplevel->scene_tree->node);
	wl_list_remove(&toplevel->link);
	wl_list_insert(&server->toplevels, &toplevel->link);
	/* Activate the new surface */
	wlr_xdg_toplevel_set_activated(toplevel->xdg_toplevel, true);
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

static void keyboard_handle_modifiers(
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
	server->cam_x += dx;
	server->cam_y += dy;
	relayout_all(server);
	update_current_star(server);
}

/* SEIZA: the operations behind every command (keys, socket, CLI). cmd.c does
 * the parsing; these do the work. `ud` is always the server. */
static void op_overview(void *ud) {
	struct tinywl_server *server = ud;
	if (server->overview) {
		fly_to_star(server, server->current_star);
	} else {
		enter_overview(server);
	}
}

static int op_new_star(void *ud) {
	struct tinywl_server *server = ud;
	fly_to_star(server, star_create(server));
	persist_save(server);
	return 0;
}

/* Drop every star marker (the windows are not touched; the caller reassigns them). */
static void stars_clear(struct tinywl_server *server) {
	struct star *s, *tmp;
	wl_list_for_each_safe(s, tmp, &server->stars, link) {
		wl_list_remove(&s->link);
		wlr_scene_node_destroy(&s->marker->node);
		free(s);
	}
	server->current_star = NULL;
	server->hover_star = NULL;
}

/* Only an empty star can go, and there is always at least one. */
static int op_delete_star(void *ud) {
	struct tinywl_server *server = ud;
	struct star *s = server->current_star;
	struct tinywl_toplevel *tl;
	if (wl_list_length(&server->stars) <= 1) {
		return -1;
	}
	wl_list_for_each(tl, &server->toplevels, link) {
		if (tl->star == s) {
			return -1;
		}
	}
	struct star *next = wl_container_of(s->link.next == &server->stars ?
		server->stars.next : s->link.next, next, link);
	wl_list_remove(&s->link);
	wlr_scene_node_destroy(&s->marker->node);
	free(s);
	server->hover_star = NULL;
	fly_to_star(server, next);
	persist_save(server);
	return 0;
}

/* A preset is a copy of the active constellation under another name. */
static int op_save(void *ud, const char *name) {
	return persist_write(ud, name, false);
}

/* Replace the active constellation with `name`. Windows move to its first star. */
static int op_load(void *ud, const char *name) {
	struct tinywl_server *server = ud;
	struct constellation c;
	if (constellation_load(server->cfg_dir, name, &c) != 0) {
		return -1;
	}
	stars_clear(server);
	persist_apply(server, &c);
	int n = 0;
	struct tinywl_toplevel *tl;
	wl_list_for_each(tl, &server->toplevels, link) {
		tl->star = server->current_star;
		tl->wx = tl->star->cx - STAR_W / 2.0 + 60 + 40 * (n % 8);
		tl->wy = tl->star->cy - STAR_H / 2.0 + 60 + 40 * (n % 8);
		n++;
	}
	constellation_set_last(server->cfg_dir, name);
	fly_to_star(server, server->current_star);
	return 0;
}

/* Defaults first, then the user's file on top. A missing file is fine. */
static void keys_load(struct tinywl_server *server) {
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
	server->keys = fresh;
}

static int op_reload_config(void *ud) {
	keys_load(ud);
	return 0;
}

static void op_quit(void *ud) {
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

/* Run through the configured shell, else ysh if installed, else /bin/sh. */
static void op_spawn(void *ud, const char *cmdline) {
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
static const struct cmd_ops seiza_ops = {
	op_overview, op_new_star, op_delete_star, op_save, op_load, op_relaunch,
	op_reload_config, op_quit, op_pan, op_zoom_reset, op_cycle_window, op_spawn,
};

/* Run the binding for this key combo, if there is one. True if it was one. */
static bool handle_keybinding(struct tinywl_server *server, uint32_t mods, xkb_keysym_t sym) {
	/* Alt+Escape always quits, even if the keys file is broken or rebinds it. */
	if (mods == KMOD_ALT && xkb_keysym_to_lower(sym) == XKB_KEY_Escape) {
		op_quit(server);
		return true;
	}
	const struct binding *b = keys_find(&server->keys, mods, sym);
	if (!b) {
		return false;
	}
	char reply[128];
	if (cmd_run(&seiza_ops, server, b->cmd, reply, sizeof(reply)) != 0) {
		wlr_log(WLR_INFO, "seiza: %s -> %s", b->cmd, reply);
	}
	return true;
}

static void keyboard_handle_key(
		struct wl_listener *listener, void *data) {
	/* This event is raised when a key is pressed or released. */
	struct tinywl_keyboard *keyboard =
		wl_container_of(listener, keyboard, key);
	struct tinywl_server *server = keyboard->server;
	struct wlr_keyboard_key_event *event = data;
	struct wlr_seat *seat = server->seat;

	/* Translate libinput keycode -> xkbcommon */
	uint32_t keycode = event->keycode + 8;
	/* Get a list of keysyms based on the keymap for this keyboard */
	const xkb_keysym_t *syms;
	int nsyms = xkb_state_key_get_syms(
			keyboard->wlr_keyboard->xkb_state, keycode, &syms);

	bool handled = false;
	uint32_t wm = wlr_keyboard_get_modifiers(keyboard->wlr_keyboard);
	uint32_t modifiers = ((wm & WLR_MODIFIER_SHIFT) ? KMOD_SHIFT : 0) |
		((wm & WLR_MODIFIER_CTRL) ? KMOD_CTRL : 0) |
		((wm & WLR_MODIFIER_ALT) ? KMOD_ALT : 0) |
		((wm & WLR_MODIFIER_LOGO) ? KMOD_SUPER : 0); /* caps/num lock don't count */
	if (event->state == WL_KEYBOARD_KEY_STATE_PRESSED) {
		/* A pressed key is tried as a compositor keybinding first. */
		for (int i = 0; i < nsyms; i++) {
			handled = handle_keybinding(server, modifiers, syms[i]) || handled;
		}
	}

	if (!handled) {
		/* Otherwise, we pass it along to the client. */
		wlr_seat_set_keyboard(seat, keyboard->wlr_keyboard);
		wlr_seat_keyboard_notify_key(seat, event->time_msec,
			event->keycode, event->state);
	}
}

static void keyboard_handle_destroy(struct wl_listener *listener, void *data) {
	/* This event is raised by the keyboard base wlr_input_device to signal
	 * the destruction of the wlr_keyboard. It will no longer receive events
	 * and should be destroyed.
	 */
	struct tinywl_keyboard *keyboard =
		wl_container_of(listener, keyboard, destroy);
	wl_list_remove(&keyboard->modifiers.link);
	wl_list_remove(&keyboard->key.link);
	wl_list_remove(&keyboard->destroy.link);
	wl_list_remove(&keyboard->link);
	free(keyboard);
}

static void server_new_keyboard(struct tinywl_server *server,
		struct wlr_input_device *device) {
	struct wlr_keyboard *wlr_keyboard = wlr_keyboard_from_input_device(device);

	struct tinywl_keyboard *keyboard = calloc(1, sizeof(*keyboard));
	keyboard->server = server;
	keyboard->wlr_keyboard = wlr_keyboard;

	/* We need to prepare an XKB keymap and assign it to the keyboard. This
	 * assumes the defaults (e.g. layout = "us"). */
	struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
	struct xkb_keymap *keymap = xkb_keymap_new_from_names(context, NULL,
		XKB_KEYMAP_COMPILE_NO_FLAGS);

	wlr_keyboard_set_keymap(wlr_keyboard, keymap);
	xkb_keymap_unref(keymap);
	xkb_context_unref(context);
	wlr_keyboard_set_repeat_info(wlr_keyboard, 25, 600);

	/* Here we set up listeners for keyboard events. */
	keyboard->modifiers.notify = keyboard_handle_modifiers;
	wl_signal_add(&wlr_keyboard->events.modifiers, &keyboard->modifiers);
	keyboard->key.notify = keyboard_handle_key;
	wl_signal_add(&wlr_keyboard->events.key, &keyboard->key);
	keyboard->destroy.notify = keyboard_handle_destroy;
	wl_signal_add(&device->events.destroy, &keyboard->destroy);

	wlr_seat_set_keyboard(server->seat, keyboard->wlr_keyboard);

	/* And add the keyboard to our list of keyboards */
	wl_list_insert(&server->keyboards, &keyboard->link);
}

static void server_new_pointer(struct tinywl_server *server,
		struct wlr_input_device *device) {
	/* We don't do anything special with pointers. All of our pointer handling
	 * is proxied through wlr_cursor. On another compositor, you might take this
	 * opportunity to do libinput configuration on the device to set
	 * acceleration, etc. */
	wlr_cursor_attach_input_device(server->cursor, device);
}

static void server_new_input(struct wl_listener *listener, void *data) {
	/* This event is raised by the backend when a new input device becomes
	 * available. */
	struct tinywl_server *server =
		wl_container_of(listener, server, new_input);
	struct wlr_input_device *device = data;
	switch (device->type) {
	case WLR_INPUT_DEVICE_KEYBOARD:
		server_new_keyboard(server, device);
		break;
	case WLR_INPUT_DEVICE_POINTER:
		server_new_pointer(server, device);
		break;
	default:
		break;
	}
	/* We need to let the wlr_seat know what our capabilities are, which is
	 * communiciated to the client. In TinyWL we always have a cursor, even if
	 * there are no pointer devices, so we always include that capability. */
	uint32_t caps = WL_SEAT_CAPABILITY_POINTER;
	if (!wl_list_empty(&server->keyboards)) {
		caps |= WL_SEAT_CAPABILITY_KEYBOARD;
	}
	wlr_seat_set_capabilities(server->seat, caps);
}

static void seat_request_cursor(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(
			listener, server, request_cursor);
	/* This event is raised by the seat when a client provides a cursor image */
	struct wlr_seat_pointer_request_set_cursor_event *event = data;
	struct wlr_seat_client *focused_client =
		server->seat->pointer_state.focused_client;
	/* This can be sent by any client, so we check to make sure this one is
	 * actually has pointer focus first. */
	if (focused_client == event->seat_client) {
		/* Once we've vetted the client, we can tell the cursor to use the
		 * provided surface as the cursor image. It will set the hardware cursor
		 * on the output that it's currently on and continue to do so as the
		 * cursor moves between outputs. */
		wlr_cursor_set_surface(server->cursor, event->surface,
				event->hotspot_x, event->hotspot_y);
	}
}

static void seat_pointer_focus_change(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(
			listener, server, pointer_focus_change);
	/* This event is raised when the pointer focus is changed, including when the
	 * client is closed. We set the cursor image to its default if target surface
	 * is NULL */
	struct wlr_seat_pointer_focus_change_event *event = data;
	if (event->new_surface == NULL) {
		wlr_cursor_set_xcursor(server->cursor, server->cursor_mgr, "default");
	}
}

static void seat_request_set_selection(struct wl_listener *listener, void *data) {
	/* This event is raised by the seat when a client wants to set the selection,
	 * usually when the user copies something. wlroots allows compositors to
	 * ignore such requests if they so choose, but in tinywl we always honor
	 */
	struct tinywl_server *server = wl_container_of(
			listener, server, request_set_selection);
	struct wlr_seat_request_set_selection_event *event = data;
	wlr_seat_set_selection(server->seat, event->source, event->serial);
}

static struct tinywl_toplevel *desktop_toplevel_at(
		struct tinywl_server *server, double lx, double ly,
		struct wlr_surface **surface, double *sx, double *sy) {
	/* SEIZA: wlr_scene_node_at tests clicks against the UNscaled surface, so it
	 * breaks when zoomed. Instead, convert the screen point to world coords and
	 * ask each window, front to back (the list is in stacking order), for the
	 * surface under that point. sx/sy come back in surface-local coords, which
	 * is exactly what clients expect, at any zoom. */
	if (server->overview) {
		return NULL; /* the overview is look-and-pick only, windows get no input */
	}
	double wxc = (lx + server->cam_x) / server->zoom;
	double wyc = (ly + server->cam_y) / server->zoom;
	struct tinywl_toplevel *tl;
	wl_list_for_each(tl, &server->toplevels, link) {
		struct wlr_surface *s = wlr_xdg_surface_surface_at(
			tl->xdg_toplevel->base, wxc - tl->wx, wyc - tl->wy, sx, sy);
		if (s) {
			*surface = s;
			return tl;
		}
	}
	return NULL;
}

static void reset_cursor_mode(struct tinywl_server *server) {
	/* SEIZA: a window dropped inside another star's territory moves to that
	 * star (judged by its center); dropped in empty space it keeps its star. */
	struct tinywl_toplevel *tl = server->grabbed_toplevel;
	if (server->cursor_mode == TINYWL_CURSOR_MOVE && tl) {
		struct wlr_box *g = &tl->xdg_toplevel->base->geometry;
		struct star *s = star_at(server, tl->wx + g->x + g->width / 2.0,
			tl->wy + g->y + g->height / 2.0);
		if (s) {
			tl->star = s;
		}
	}
	if (server->cursor_mode == TINYWL_CURSOR_PAN) {
		update_current_star(server);
	}
	/* Reset the cursor mode to passthrough. */
	server->cursor_mode = TINYWL_CURSOR_PASSTHROUGH;
	server->grabbed_toplevel = NULL;
}

static void process_cursor_move(struct tinywl_server *server) {
	/* Move the grabbed toplevel to the new position. */
	struct tinywl_toplevel *toplevel = server->grabbed_toplevel;
	toplevel->wx = (server->cursor->x + server->cam_x) / server->zoom - server->grab_x;
	toplevel->wy = (server->cursor->y + server->cam_y) / server->zoom - server->grab_y;
	layout_toplevel(toplevel);
}

static void process_cursor_resize(struct tinywl_server *server) {
	/*
	 * Resizing the grabbed toplevel can be a little bit complicated, because we
	 * could be resizing from any corner or edge. This not only resizes the
	 * toplevel on one or two axes, but can also move the toplevel if you resize
	 * from the top or left edges (or top-left corner).
	 *
	 * Note that some shortcuts are taken here. In a more fleshed-out
	 * compositor, you'd wait for the client to prepare a buffer at the new
	 * size, then commit any movement that was prepared.
	 */
	struct tinywl_toplevel *toplevel = server->grabbed_toplevel;
	double border_x = (server->cursor->x + server->cam_x) / server->zoom - server->grab_x;
	double border_y = (server->cursor->y + server->cam_y) / server->zoom - server->grab_y;
	int new_left = server->grab_geobox.x;
	int new_right = server->grab_geobox.x + server->grab_geobox.width;
	int new_top = server->grab_geobox.y;
	int new_bottom = server->grab_geobox.y + server->grab_geobox.height;

	if (server->resize_edges & WLR_EDGE_TOP) {
		new_top = border_y;
		if (new_bottom - new_top < MIN_WIN_H) {
			new_top = new_bottom - MIN_WIN_H;
		}
	} else if (server->resize_edges & WLR_EDGE_BOTTOM) {
		new_bottom = border_y;
		if (new_bottom - new_top < MIN_WIN_H) {
			new_bottom = new_top + MIN_WIN_H;
		}
	}
	if (server->resize_edges & WLR_EDGE_LEFT) {
		new_left = border_x;
		if (new_right - new_left < MIN_WIN_W) {
			new_left = new_right - MIN_WIN_W;
		}
	} else if (server->resize_edges & WLR_EDGE_RIGHT) {
		new_right = border_x;
		if (new_right - new_left < MIN_WIN_W) {
			new_right = new_left + MIN_WIN_W;
		}
	}

	struct wlr_box *geo_box = &toplevel->xdg_toplevel->base->geometry;
	toplevel->wx = new_left - geo_box->x;
	toplevel->wy = new_top - geo_box->y;
	layout_toplevel(toplevel);

	int new_width = new_right - new_left;
	int new_height = new_bottom - new_top;
	wlr_xdg_toplevel_set_size(toplevel->xdg_toplevel, new_width, new_height);
}

static void process_cursor_motion(struct tinywl_server *server, uint32_t time) {
	/* SEIZA: in the overview the pointer only picks stars. */
	if (server->overview) {
		wlr_cursor_set_xcursor(server->cursor, server->cursor_mgr, "default");
		overview_pointer(server);
		return;
	}
	/* SEIZA: dragging the canvas: the world follows the cursor like a map. */
	if (server->cursor_mode == TINYWL_CURSOR_PAN) {
		server->cam_x -= server->cursor->x - server->pan_last_x;
		server->cam_y -= server->cursor->y - server->pan_last_y;
		server->pan_last_x = server->cursor->x;
		server->pan_last_y = server->cursor->y;
		relayout_all(server);
		return;
	}
	/* If the mode is non-passthrough, delegate to those functions. */
	if (server->cursor_mode == TINYWL_CURSOR_MOVE) {
		process_cursor_move(server);
		return;
	} else if (server->cursor_mode == TINYWL_CURSOR_RESIZE) {
		process_cursor_resize(server);
		return;
	}

	/* Otherwise, find the toplevel under the pointer and send the event along. */
	double sx, sy;
	struct wlr_seat *seat = server->seat;
	struct wlr_surface *surface = NULL;
	struct tinywl_toplevel *toplevel = desktop_toplevel_at(server,
			server->cursor->x, server->cursor->y, &surface, &sx, &sy);
	if (!toplevel) {
		/* If there's no toplevel under the cursor, set the cursor image to a
		 * default. This is what makes the cursor image appear when you move it
		 * around the screen, not over any toplevels. */
		wlr_cursor_set_xcursor(server->cursor, server->cursor_mgr, "default");
	}
	if (surface) {
		/*
		 * Send pointer enter and motion events.
		 *
		 * The enter event gives the surface "pointer focus", which is distinct
		 * from keyboard focus. You get pointer focus by moving the pointer over
		 * a window.
		 *
		 * Note that wlroots will avoid sending duplicate enter/motion events if
		 * the surface has already has pointer focus or if the client is already
		 * aware of the coordinates passed.
		 */
		wlr_seat_pointer_notify_enter(seat, surface, sx, sy);
		wlr_seat_pointer_notify_motion(seat, time, sx, sy);
	} else {
		/* Clear pointer focus so future button events and such are not sent to
		 * the last client to have the cursor over it. */
		wlr_seat_pointer_clear_focus(seat);
	}
}

static void server_cursor_motion(struct wl_listener *listener, void *data) {
	/* This event is forwarded by the cursor when a pointer emits a _relative_
	 * pointer motion event (i.e. a delta) */
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_motion);
	struct wlr_pointer_motion_event *event = data;
	/* The cursor doesn't move unless we tell it to. The cursor automatically
	 * handles constraining the motion to the output layout, as well as any
	 * special configuration applied for the specific input device which
	 * generated the event. You can pass NULL for the device if you want to move
	 * the cursor around without any input. */
	wlr_cursor_move(server->cursor, &event->pointer->base,
			event->delta_x, event->delta_y);
	process_cursor_motion(server, event->time_msec);
}

static void server_cursor_motion_absolute(
		struct wl_listener *listener, void *data) {
	/* This event is forwarded by the cursor when a pointer emits an _absolute_
	 * motion event, from 0..1 on each axis. This happens, for example, when
	 * wlroots is running under a Wayland window rather than KMS+DRM, and you
	 * move the mouse over the window. You could enter the window from any edge,
	 * so we have to warp the mouse there. There is also some hardware which
	 * emits these events. */
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_motion_absolute);
	struct wlr_pointer_motion_absolute_event *event = data;
	wlr_cursor_warp_absolute(server->cursor, &event->pointer->base, event->x,
		event->y);
	process_cursor_motion(server, event->time_msec);
}

static void begin_interactive(struct tinywl_toplevel *toplevel,
		enum tinywl_cursor_mode mode, uint32_t edges);

static void server_cursor_button(struct wl_listener *listener, void *data) {
	/* This event is forwarded by the cursor when a pointer emits a button
	 * event. */
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_button);
	struct wlr_pointer_button_event *event = data;

	/* SEIZA: in the overview a click on a star flies into it at once. */
	if (server->overview) {
		if (event->state == WL_POINTER_BUTTON_STATE_PRESSED) {
			struct star *s = star_at(server,
				(server->cursor->x + server->cam_x) / server->zoom,
				(server->cursor->y + server->cam_y) / server->zoom);
			if (s) {
				fly_to_star(server, s);
			}
		}
		return;
	}

	/* SEIZA: Alt + left-drag moves the window under the cursor, Alt +
	 * right-drag resizes it from the nearest corner, even when the app draws
	 * no title bar. The click is not forwarded to the app. */
	struct wlr_keyboard *kb = wlr_seat_get_keyboard(server->seat);
	if (event->state == WL_POINTER_BUTTON_STATE_PRESSED &&
			(event->button == BTN_LEFT || event->button == BTN_RIGHT) &&
			kb && (wlr_keyboard_get_modifiers(kb) & WLR_MODIFIER_ALT)) {
		double sx, sy;
		struct wlr_surface *surface = NULL;
		struct tinywl_toplevel *toplevel = desktop_toplevel_at(server,
				server->cursor->x, server->cursor->y, &surface, &sx, &sy);
		if (toplevel) {
			focus_toplevel(toplevel);
			if (event->button == BTN_LEFT) {
				begin_interactive(toplevel, TINYWL_CURSOR_MOVE, 0);
			} else {
				/* Pick the corner by which quadrant of the window was hit. */
				struct wlr_box *geo = &toplevel->xdg_toplevel->base->geometry;
				double mid_x = (toplevel->wx + geo->x + geo->width / 2.0) * server->zoom - server->cam_x;
				double mid_y = (toplevel->wy + geo->y + geo->height / 2.0) * server->zoom - server->cam_y;
				uint32_t edges = (server->cursor->x < mid_x ? WLR_EDGE_LEFT : WLR_EDGE_RIGHT) |
					(server->cursor->y < mid_y ? WLR_EDGE_TOP : WLR_EDGE_BOTTOM);
				begin_interactive(toplevel, TINYWL_CURSOR_RESIZE, edges);
			}
			return;
		}
		/* Alt-click on empty space: grab the canvas and drag it. */
		server->cursor_mode = TINYWL_CURSOR_PAN;
		server->pan_last_x = server->cursor->x;
		server->pan_last_y = server->cursor->y;
		return;
	}
	/* Notify the client with pointer focus that a button press has occurred */
	wlr_seat_pointer_notify_button(server->seat,
			event->time_msec, event->button, event->state);
	if (event->state == WL_POINTER_BUTTON_STATE_RELEASED) {
		/* If you released any buttons, we exit interactive move/resize mode. */
		reset_cursor_mode(server);
	} else {
		/* Focus that client if the button was _pressed_ */
		double sx, sy;
		struct wlr_surface *surface = NULL;
		struct tinywl_toplevel *toplevel = desktop_toplevel_at(server,
				server->cursor->x, server->cursor->y, &surface, &sx, &sy);
		focus_toplevel(toplevel);
	}
}

static void server_cursor_axis(struct wl_listener *listener, void *data) {
	/* This event is forwarded by the cursor when a pointer emits an axis event,
	 * for example when you move the scroll wheel. */
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_axis);
	struct wlr_pointer_axis_event *event = data;

	/* SEIZA: Alt + scroll zooms toward the cursor (scroll up = zoom in) and is
	 * not forwarded to the app. */
	if (alt_held(server) && event->orientation == WL_POINTER_AXIS_VERTICAL_SCROLL) {
		zoom_at(server, event->delta < 0 ? 1.1 : 1 / 1.1);
		return;
	}
	/* Notify the client with pointer focus of the axis event. */
	wlr_seat_pointer_notify_axis(server->seat,
			event->time_msec, event->orientation, event->delta,
			event->delta_discrete, event->source, event->relative_direction);
}

static void server_cursor_frame(struct wl_listener *listener, void *data) {
	/* This event is forwarded by the cursor when a pointer emits an frame
	 * event. Frame events are sent after regular pointer events to group
	 * multiple events together. For instance, two axis events may happen at the
	 * same time, in which case a frame event won't be sent in between. */
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_frame);
	/* Notify the client with pointer focus of the frame event. */
	wlr_seat_pointer_notify_frame(server->seat);
}

static void output_frame(struct wl_listener *listener, void *data) {
	/* This function is called every time an output is ready to display a frame,
	 * generally at the output's refresh rate (e.g. 60Hz). */
	struct tinywl_output *output = wl_container_of(listener, output, frame);
	struct wlr_scene *scene = output->server->scene;

	struct wlr_scene_output *scene_output = wlr_scene_get_scene_output(
		scene, output->wlr_output);

	/* Render the scene if needed and commit the output */
	wlr_scene_output_commit(scene_output, NULL);

	struct timespec now;
	clock_gettime(CLOCK_MONOTONIC, &now);
	wlr_scene_output_send_frame_done(scene_output, &now);
}

static void output_request_state(struct wl_listener *listener, void *data) {
	/* This function is called when the backend requests a new state for
	 * the output. For example, Wayland and X11 backends request a new mode
	 * when the output window is resized. */
	struct tinywl_output *output = wl_container_of(listener, output, request_state);
	const struct wlr_output_event_request_state *event = data;
	wlr_output_commit_state(output->wlr_output, event->state);
}

static void output_destroy(struct wl_listener *listener, void *data) {
	struct tinywl_output *output = wl_container_of(listener, output, destroy);

	wl_list_remove(&output->frame.link);
	wl_list_remove(&output->request_state.link);
	wl_list_remove(&output->destroy.link);
	wl_list_remove(&output->link);
	free(output);
}

static void server_new_output(struct wl_listener *listener, void *data) {
	/* This event is raised by the backend when a new output (aka a display or
	 * monitor) becomes available. */
	struct tinywl_server *server =
		wl_container_of(listener, server, new_output);
	struct wlr_output *wlr_output = data;

	/* Configures the output created by the backend to use our allocator
	 * and our renderer. Must be done once, before committing the output */
	wlr_output_init_render(wlr_output, server->allocator, server->renderer);

	/* The output may be disabled, switch it on. */
	struct wlr_output_state state;
	wlr_output_state_init(&state);
	wlr_output_state_set_enabled(&state, true);

	/* Some backends don't have modes. DRM+KMS does, and we need to set a mode
	 * before we can use the output. The mode is a tuple of (width, height,
	 * refresh rate), and each monitor supports only a specific set of modes. We
	 * just pick the monitor's preferred mode, a more sophisticated compositor
	 * would let the user configure it. */
	struct wlr_output_mode *mode = wlr_output_preferred_mode(wlr_output);
	if (mode != NULL) {
		wlr_output_state_set_mode(&state, mode);
	}

	/* Atomically applies the new output state. */
	wlr_output_commit_state(wlr_output, &state);
	wlr_output_state_finish(&state);

	/* Allocates and configures our state for this output */
	struct tinywl_output *output = calloc(1, sizeof(*output));
	output->wlr_output = wlr_output;
	output->server = server;

	/* Sets up a listener for the frame event. */
	output->frame.notify = output_frame;
	wl_signal_add(&wlr_output->events.frame, &output->frame);

	/* Sets up a listener for the state request event. */
	output->request_state.notify = output_request_state;
	wl_signal_add(&wlr_output->events.request_state, &output->request_state);

	/* Sets up a listener for the destroy event. */
	output->destroy.notify = output_destroy;
	wl_signal_add(&wlr_output->events.destroy, &output->destroy);

	wl_list_insert(&server->outputs, &output->link);

	/* Adds this to the output layout. The add_auto function arranges outputs
	 * from left-to-right in the order they appear. A more sophisticated
	 * compositor would let the user configure the arrangement of outputs in the
	 * layout.
	 *
	 * The output layout utility automatically adds a wl_output global to the
	 * display, which Wayland clients can see to find out information about the
	 * output (such as DPI, scale factor, manufacturer, etc).
	 */
	struct wlr_output_layout_output *l_output = wlr_output_layout_add_auto(server->output_layout,
		wlr_output);
	struct wlr_scene_output *scene_output = wlr_scene_output_create(server->scene, wlr_output);
	wlr_scene_output_layout_add_output(server->scene_layout, l_output, scene_output);
}

/* Remember how this window's program was launched, on its star, so the star can
 * offer to relaunch it later. Closing the window does not forget it. */
static void remember_app(struct tinywl_toplevel *tl) {
	struct star *s = tl->star;
	pid_t pid;
	char cmd[256];
	wl_client_get_credentials(wl_resource_get_client(tl->xdg_toplevel->base->resource),
		&pid, NULL, NULL);
	if (procinfo_cmdline(pid, cmd, sizeof(cmd)) != 0) {
		return;
	}
	struct cstar view = {.napps = s->napps}; /* reuse the de-dupe + cap logic */
	memcpy(view.apps, s->apps, sizeof(view.apps));
	if (constellation_add_app(&view, cmd)) {
		s->napps = view.napps;
		memcpy(s->apps, view.apps, sizeof(s->apps));
		persist_save(tl->server);
	}
}

static void xdg_toplevel_map(struct wl_listener *listener, void *data) {
	/* Called when the surface is mapped, or ready to display on-screen. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, map);

	wl_list_insert(&toplevel->server->toplevels, &toplevel->link);
	remember_app(toplevel);

	focus_toplevel(toplevel);
}

static void xdg_toplevel_unmap(struct wl_listener *listener, void *data) {
	/* Called when the surface is unmapped, and should no longer be shown. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, unmap);

	/* Reset the cursor mode if the grabbed toplevel was unmapped. */
	if (toplevel == toplevel->server->grabbed_toplevel) {
		reset_cursor_mode(toplevel->server);
	}

	wl_list_remove(&toplevel->link);
}

static void xdg_toplevel_commit(struct wl_listener *listener, void *data) {
	/* Called when a new surface state is committed. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, commit);

	if (toplevel->xdg_toplevel->base->initial_commit) {
		/* When an xdg_surface performs an initial commit, the compositor must
		 * reply with a configure so the client can map the surface. tinywl
		 * configures the xdg_toplevel with 0,0 size to let the client pick the
		 * dimensions itself. */
		wlr_xdg_toplevel_set_size(toplevel->xdg_toplevel, 0, 0);
	}
	/* SEIZA: wlroots just reset this window's drawn size, and its geometry may
	 * have changed, so re-apply zoom + frame. (Our listener is added after
	 * wlroots', so it runs after it.) */
	layout_toplevel(toplevel);
}

static void xdg_toplevel_destroy(struct wl_listener *listener, void *data) {
	/* Called when the xdg_toplevel is destroyed. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, destroy);

	wl_list_remove(&toplevel->map.link);
	wl_list_remove(&toplevel->unmap.link);
	wl_list_remove(&toplevel->commit.link);
	wl_list_remove(&toplevel->destroy.link);
	wl_list_remove(&toplevel->request_move.link);
	wl_list_remove(&toplevel->request_resize.link);
	wl_list_remove(&toplevel->request_maximize.link);
	wl_list_remove(&toplevel->request_fullscreen.link);

	free(toplevel);
}

static void begin_interactive(struct tinywl_toplevel *toplevel,
		enum tinywl_cursor_mode mode, uint32_t edges) {
	/* This function sets up an interactive move or resize operation, where the
	 * compositor stops propagating pointer events to clients and instead
	 * consumes them itself, to move or resize windows. */
	struct tinywl_server *server = toplevel->server;

	server->grabbed_toplevel = toplevel;
	server->cursor_mode = mode;

	/* SEIZA: all grab math is in world coordinates. */
	double cur_wx = (server->cursor->x + server->cam_x) / server->zoom;
	double cur_wy = (server->cursor->y + server->cam_y) / server->zoom;

	if (mode == TINYWL_CURSOR_MOVE) {
		server->grab_x = cur_wx - toplevel->wx;
		server->grab_y = cur_wy - toplevel->wy;
	} else {
		struct wlr_box *geo_box = &toplevel->xdg_toplevel->base->geometry;

		double border_x = (toplevel->wx + geo_box->x) +
			((edges & WLR_EDGE_RIGHT) ? geo_box->width : 0);
		double border_y = (toplevel->wy + geo_box->y) +
			((edges & WLR_EDGE_BOTTOM) ? geo_box->height : 0);
		server->grab_x = cur_wx - border_x;
		server->grab_y = cur_wy - border_y;

		server->grab_geobox = *geo_box;
		server->grab_geobox.x += lround(toplevel->wx);
		server->grab_geobox.y += lround(toplevel->wy);

		server->resize_edges = edges;
	}
}

static void xdg_toplevel_request_move(
		struct wl_listener *listener, void *data) {
	/* This event is raised when a client would like to begin an interactive
	 * move, typically because the user clicked on their client-side
	 * decorations. Note that a more sophisticated compositor should check the
	 * provided serial against a list of button press serials sent to this
	 * client, to prevent the client from requesting this whenever they want. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, request_move);
	begin_interactive(toplevel, TINYWL_CURSOR_MOVE, 0);
}

static void xdg_toplevel_request_resize(
		struct wl_listener *listener, void *data) {
	/* This event is raised when a client would like to begin an interactive
	 * resize, typically because the user clicked on their client-side
	 * decorations. Note that a more sophisticated compositor should check the
	 * provided serial against a list of button press serials sent to this
	 * client, to prevent the client from requesting this whenever they want. */
	struct wlr_xdg_toplevel_resize_event *event = data;
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, request_resize);
	begin_interactive(toplevel, TINYWL_CURSOR_RESIZE, event->edges);
}

static void xdg_toplevel_request_maximize(
		struct wl_listener *listener, void *data) {
	/* This event is raised when a client would like to maximize itself,
	 * typically because the user clicked on the maximize button on client-side
	 * decorations. tinywl doesn't support maximization, but to conform to
	 * xdg-shell protocol we still must send a configure.
	 * wlr_xdg_surface_schedule_configure() is used to send an empty reply.
	 * However, if the request was sent before an initial commit, we don't do
	 * anything and let the client finish the initial surface setup. */
	struct tinywl_toplevel *toplevel =
		wl_container_of(listener, toplevel, request_maximize);
	if (toplevel->xdg_toplevel->base->initialized) {
		wlr_xdg_surface_schedule_configure(toplevel->xdg_toplevel->base);
	}
}

static void xdg_toplevel_request_fullscreen(
		struct wl_listener *listener, void *data) {
	/* Just as with request_maximize, we must send a configure here. */
	struct tinywl_toplevel *toplevel =
		wl_container_of(listener, toplevel, request_fullscreen);
	if (toplevel->xdg_toplevel->base->initialized) {
		wlr_xdg_surface_schedule_configure(toplevel->xdg_toplevel->base);
	}
}

static void server_new_xdg_toplevel(struct wl_listener *listener, void *data) {
	/* This event is raised when a client creates a new toplevel (application window). */
	struct tinywl_server *server = wl_container_of(listener, server, new_xdg_toplevel);
	struct wlr_xdg_toplevel *xdg_toplevel = data;

	/* Allocate a tinywl_toplevel for this surface */
	struct tinywl_toplevel *toplevel = calloc(1, sizeof(*toplevel));
	toplevel->server = server;
	toplevel->xdg_toplevel = xdg_toplevel;
	toplevel->scene_tree =
		wlr_scene_xdg_surface_create(toplevel->server->canvas, xdg_toplevel->base);
	toplevel->scene_tree->node.data = toplevel;
	xdg_toplevel->base->data = toplevel->scene_tree;

	for (int i = 0; i < 4; i++) {
		toplevel->border[i] = wlr_scene_rect_create(toplevel->scene_tree, 0, 0, BORDER_IDLE);
	}

	/* SEIZA: a new window lands in the star you are currently in, cascaded from
	 * that star's top-left so windows don't open stacked exactly on top of each
	 * other. */
	toplevel->star = server->current_star;
	int n = 0;
	struct tinywl_toplevel *other;
	wl_list_for_each(other, &server->toplevels, link) {
		n += other->star == toplevel->star;
	}
	n %= 8;
	toplevel->wx = toplevel->star->cx - STAR_W / 2.0 + 60 + 40 * n;
	toplevel->wy = toplevel->star->cy - STAR_H / 2.0 + 60 + 40 * n;
	layout_toplevel(toplevel);

	/* Listen to the various events it can emit */
	toplevel->map.notify = xdg_toplevel_map;
	wl_signal_add(&xdg_toplevel->base->surface->events.map, &toplevel->map);
	toplevel->unmap.notify = xdg_toplevel_unmap;
	wl_signal_add(&xdg_toplevel->base->surface->events.unmap, &toplevel->unmap);
	toplevel->commit.notify = xdg_toplevel_commit;
	wl_signal_add(&xdg_toplevel->base->surface->events.commit, &toplevel->commit);

	toplevel->destroy.notify = xdg_toplevel_destroy;
	wl_signal_add(&xdg_toplevel->events.destroy, &toplevel->destroy);

	/* cotd */
	toplevel->request_move.notify = xdg_toplevel_request_move;
	wl_signal_add(&xdg_toplevel->events.request_move, &toplevel->request_move);
	toplevel->request_resize.notify = xdg_toplevel_request_resize;
	wl_signal_add(&xdg_toplevel->events.request_resize, &toplevel->request_resize);
	toplevel->request_maximize.notify = xdg_toplevel_request_maximize;
	wl_signal_add(&xdg_toplevel->events.request_maximize, &toplevel->request_maximize);
	toplevel->request_fullscreen.notify = xdg_toplevel_request_fullscreen;
	wl_signal_add(&xdg_toplevel->events.request_fullscreen, &toplevel->request_fullscreen);
}

static void xdg_popup_commit(struct wl_listener *listener, void *data) {
	/* Called when a new surface state is committed. */
	struct tinywl_popup *popup = wl_container_of(listener, popup, commit);

	if (popup->xdg_popup->base->initial_commit) {
		/* When an xdg_surface performs an initial commit, the compositor must
		 * reply with a configure so the client can map the surface.
		 * tinywl sends an empty configure. A more sophisticated compositor
		 * might change an xdg_popup's geometry to ensure it's not positioned
		 * off-screen, for example. */
		wlr_xdg_surface_schedule_configure(popup->xdg_popup->base);
	}
}

static void xdg_popup_destroy(struct wl_listener *listener, void *data) {
	/* Called when the xdg_popup is destroyed. */
	struct tinywl_popup *popup = wl_container_of(listener, popup, destroy);

	wl_list_remove(&popup->commit.link);
	wl_list_remove(&popup->destroy.link);

	free(popup);
}

static void server_new_xdg_popup(struct wl_listener *listener, void *data) {
	/* This event is raised when a client creates a new popup. */
	struct wlr_xdg_popup *xdg_popup = data;

	struct tinywl_popup *popup = calloc(1, sizeof(*popup));
	popup->xdg_popup = xdg_popup;

	/* We must add xdg popups to the scene graph so they get rendered. The
	 * wlroots scene graph provides a helper for this, but to use it we must
	 * provide the proper parent scene node of the xdg popup. To enable this,
	 * we always set the user data field of xdg_surfaces to the corresponding
	 * scene node. */
	struct wlr_xdg_surface *parent = wlr_xdg_surface_try_from_wlr_surface(xdg_popup->parent);
	assert(parent != NULL);
	struct wlr_scene_tree *parent_tree = parent->data;
	xdg_popup->base->data = wlr_scene_xdg_surface_create(parent_tree, xdg_popup->base);

	popup->commit.notify = xdg_popup_commit;
	wl_signal_add(&xdg_popup->base->surface->events.commit, &popup->commit);

	popup->destroy.notify = xdg_popup_destroy;
	wl_signal_add(&xdg_popup->events.destroy, &popup->destroy);
}

int main(int argc, char *argv[]) {
	/* `seiza <command>` from a shell talks to the running compositor. */
	if (argc > 1 && argv[1][0] != '-') {
		return ipc_client(argc, argv);
	}
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
	server.renderer = wlr_renderer_autocreate(server.backend);
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
	wlr_compositor_create(server.wl_display, 5, server.renderer);
	wlr_subcompositor_create(server.wl_display);
	wlr_data_device_manager_create(server.wl_display);

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
	server.canvas = wlr_scene_tree_create(&server.scene->tree);
	server.zoom = 1.0;
	wl_list_init(&server.stars);
	server.hover_timer = wl_event_loop_add_timer(
		wl_display_get_event_loop(server.wl_display), hover_timeout, &server);
	/* Start in star 0: its territory is centered so the default camera
	 * (0,0) already looks at it on a 1920x1080 screen. */
	persist_startup(&server);
	keys_load(&server);

	/* Set up xdg-shell version 3. The xdg-shell is a Wayland protocol which is
	 * used for application windows. For more detail on shells, refer to
	 * https://drewdevault.com/2018/07/29/Wayland-shells.html.
	 */
	wl_list_init(&server.toplevels);
	fly_to_star(&server, server.current_star); /* restored stars may not sit at the origin */
	server.xdg_shell = wlr_xdg_shell_create(server.wl_display, 3);
	server.new_xdg_toplevel.notify = server_new_xdg_toplevel;
	wl_signal_add(&server.xdg_shell->events.new_toplevel, &server.new_xdg_toplevel);
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
	ipc_serve(wl_display_get_event_loop(server.wl_display), &seiza_ops, &server);
	wl_display_run(server.wl_display);
	ipc_close();


	/* Once wl_display_run returns, we destroy all clients then shut down the
	 * server. */
	wl_display_destroy_clients(server.wl_display);

	wl_list_remove(&server.new_xdg_toplevel.link);
	wl_list_remove(&server.new_xdg_popup.link);

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

	wlr_scene_node_destroy(&server.scene->tree.node);
	wlr_xcursor_manager_destroy(server.cursor_mgr);
	wlr_cursor_destroy(server.cursor);
	wlr_allocator_destroy(server.allocator);
	wlr_renderer_destroy(server.renderer);
	wlr_backend_destroy(server.backend);
	wl_display_destroy(server.wl_display);
	return 0;
}
