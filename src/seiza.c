/* SEIZA: promoted from spike/ on 2026-10-22 (design: docs/2026-10-22-constellations-design.md) */
#include <assert.h>
#include <getopt.h>
#include <stdbool.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <stdio.h>
#include <sys/wait.h>
#include <signal.h>
#include <time.h>
#include <unistd.h>
#include <wayland-server-core.h>
#include <wlr/backend.h>
#include <wlr/render/allocator.h>
#include <wlr/render/wlr_renderer.h>
#include <drm_fourcc.h>
#include <wlr/interfaces/wlr_buffer.h>
#include <wlr/types/wlr_buffer.h>
#include <wlr/types/wlr_cursor.h>
#include <wlr/types/wlr_compositor.h>
#include <wlr/types/wlr_data_control_v1.h>
#include <wlr/types/wlr_screencopy_v1.h>
#include <wlr/types/wlr_pointer_constraints_v1.h>
#include <wlr/types/wlr_relative_pointer_v1.h>
#include <wlr/types/wlr_ext_image_copy_capture_v1.h>
#include <wlr/types/wlr_ext_image_capture_source_v1.h>
#include <wlr/types/wlr_data_device.h>
#include <wlr/types/wlr_input_device.h>
#include <wlr/types/wlr_layer_shell_v1.h>
#include <linux/input-event-codes.h>
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_output_layout.h>
#include <wlr/types/wlr_pointer.h>
#include <scenefx/types/wlr_scene.h>
#include <scenefx/render/fx_renderer/fx_renderer.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_session_lock_v1.h>
#include <wlr/types/wlr_idle_inhibit_v1.h>
#include <wlr/xwayland.h>
#include <wlr/types/wlr_viewporter.h>
#include <wlr/types/wlr_xdg_output_v1.h>
#include <wlr/types/wlr_subcompositor.h>
#include <wlr/types/wlr_xcursor_manager.h>
#include <wlr/types/wlr_xdg_shell.h>
#include <wlr/types/wlr_xdg_decoration_v1.h>
#include <wlr/util/log.h>
#include <xkbcommon/xkbcommon.h>
#include <sys/inotify.h>
#include "cmd.h"
#include "constellation.h"
#include "ipc.h"
#include "procinfo.h"
#include "keys.h"
#include "palette.h"
#include "motion.h"
#include "night.h"
#include "frame.h"
#include "figure.h"

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
	int tiled; /* 0 follows the layout config, 1 tiled, -1 floating (Super+t) */
};

/* An X11 override-redirect window: a menu, a tooltip, a drop-down. The program places it itself,
 * in X coordinates, which are relative to its window's origin (every managed window sits at X
 * (0,0)); so it is drawn at that window's place on the canvas plus those coordinates, and nobody
 * manages it. */
struct xunmanaged {
	struct wl_list link;
	struct tinywl_server *server;
	struct wlr_xwayland_surface *xs;
	struct wlr_scene_tree *tree;
	struct wlr_scene_surface *surf; /* while mapped */
	bool attached;                  /* map/unmap are listened to on the X11 surface */
	bool committing;                /* so is commit, while it is shown */
	double ax, ay;                  /* the world place of the window it belongs to, taken when it was shown */
	struct wl_listener associate, dissociate, destroy, map, unmap, set_geometry, request_configure, commit;
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
	struct {                   /* a camera glide: world-space centre and zoom, from -> to */
		bool on;
		double t0, dur, cx0, cy0, z0, cx1, cy1, z1;
	} anim;
	double press_x, press_y;   /* where a star was pressed in the overview */
	bool drag_moved;           /* it moved far enough to be a drag, not a click */
	struct star *drag_star;    /* star being moved with Shift+drag in the overview */
	double drag_x0, drag_y0;   /* where it started, to revert an overlapping drop */

	/* SEIZA: layer-shell surfaces (bars, launchers, wallpapers) are screen-fixed,
	 * not part of the canvas: they live in trees around it, in protocol order
	 * (background, bottom, [canvas], top, overlay). */
	struct wlr_scene_tree *layers[4];
	/* SEIZA: ext-session-lock. While locked, a black rect and the lock client's
	 * surfaces cover everything and all input goes to the lock client. */
	struct wlr_session_lock_manager_v1 *lock_mgr;
	struct wl_listener new_lock;
	struct wlr_session_lock_v1 *lock;  /* the live lock client; NULL if none or it died */
	struct wl_listener lock_new_surface, lock_unlock, lock_destroy;
	struct wlr_scene_tree *lock_tree;
	struct wlr_scene_rect *lock_bg;
	bool locked;
	int links[CONST_LINKS_MAX][2]; /* threads between stars, as indexes in creation order */
	int nlinks;
	struct wlr_xwayland *xwayland;
	struct wl_listener new_xwayland_surface, xwayland_ready;
	struct wl_list unmanaged; /* struct xunmanaged */
	struct wlr_scene_tree *xtop; /* in the canvas, raised above the windows: where those menus draw */
	int idle_dim, idle_lock, idle_off; /* seconds without input until the screen dims / seiza-lock / screens off; 0 = never. From seiza.kdl */
	int panel_corner, panel_dwell; /* hot corner that opens seiza-panel: 0 off, 1 top-right, 2 top-left, 3 bottom-right, 4 bottom-left; dwell in ms. From seiza.kdl */
	struct wlr_idle_inhibit_manager_v1 *idle_inhibit; /* a client holding an inhibitor (a video) keeps the screen awake */
	struct wlr_scene_rect *dim_rect;
	struct motion motion; /* pan duration and easing, from ~/.config/seiza/motion */
	struct wlr_box full, usable; /* the output, and what the layer surfaces (the bar) leave of it; set by layers_arrange */
	struct wlr_scene_tree *corner_tree; /* above everything: black corner masks make the screen rounded */
	struct wlr_layer_shell_v1 *layer_shell;
	struct wl_listener new_layer_surface;
	struct wl_list layer_surfaces;

	struct wlr_xdg_shell *xdg_shell;
	struct wl_listener new_xdg_toplevel;
	struct wl_listener new_xdg_popup;
	struct wl_list toplevels;     /* mapped windows, front to back */
	struct wl_list all_toplevels; /* SEIZA: every window that exists; each holds a star pointer */

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
	struct wlr_pointer_constraints_v1 *constraints;
	struct wlr_relative_pointer_manager_v1 *relative_pointer;
	struct wlr_pointer_constraint_v1 *constraint; /* the one in force on the pointer-focused surface, or NULL */
	struct wl_listener new_constraint, constraint_destroy;
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
	struct wlr_scene_buffer *corner[4]; /* SEIZA: the screen's rounded corners, TL TR BL BR */
};

struct tinywl_toplevel {
	struct wl_list link;
	struct wl_list all_link; /* SEIZA: in server->all_toplevels from creation to destroy, mapped or not */
	struct tinywl_server *server;
	struct wlr_xdg_toplevel *xdg_toplevel; /* a Wayland window... */
	struct wlr_xwayland_surface *xsurface; /* ...or an X11 one (then xdg_toplevel is NULL) */
	struct wlr_scene_surface *x_scene;     /* the X11 window's surface in the scene, while it is mapped */
	bool x_attached;                       /* map/unmap/commit are listened to on the X11 window's surface */
	struct wl_listener associate, dissociate, request_configure, request_activate; /* X11 only */
	struct wlr_scene_tree *scene_tree;
	double wx, wy; /* SEIZA: window origin in world coordinates */
	struct star *star; /* SEIZA: the workspace this window belongs to */
	/* SEIZA window frame: a rounded plate behind the window (only its rim shows) and a bloom while focused. */
	struct wlr_scene_rect *border;
	struct wlr_scene_shadow *glow;
	int mode; /* 0 normal, 1 maximized */
	float faded;    /* 0 when fully shown; while it fades in, how much is still missing (the frame follows it) */
	double open_t0; /* when it was mapped, while it is still fading in; 0 otherwise */
	double saved_wx, saved_wy; /* where it was, and how big, before it left mode 0 */
	int saved_w, saved_h;
	struct wl_listener map;
	struct wl_listener unmap;
	struct wl_listener commit;
	struct wl_listener destroy;
	struct wl_listener request_move;
	struct wl_listener request_resize;
	struct wl_listener request_maximize;
	struct wl_listener request_fullscreen;
};

struct seiza_layer {
	struct wl_list link;
	struct tinywl_server *server;
	struct wlr_layer_surface_v1 *surf;
	struct wlr_scene_layer_surface_v1 *scene;
	struct wl_listener commit, map, unmap, destroy;
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

/* SEIZA window frame: a rounded plate behind the window (only its rim shows)
 * plus a soft gold bloom while focused. Thickness and radius are in screen px
 * at zoom 1; the radius follows the zoom, the thickness does not. */
#define GLOW_SIGMA 18
#define OPEN_POPIN 0.2 /* Hyprland's `popin 80%`: a window opens at 80% size and grows to 100% along its fade */
static struct frame FRAME; /* radius, border and glow: from the frame file, set by frame_colors_load() */
#define WIN_RADIUS FRAME.radius
#define BORDER_W FRAME.border
static float BORDER_FOCUSED[4], BORDER_IDLE[4], GLOW_FOCUSED[4]; /* set by frame_colors_load() */
static const float GLOW_IDLE[4]      = {0, 0, 0, 0};

/* A window is a Wayland one (xdg) or an X11 one (XWayland); the rest of the compositor asks these. */
static struct wlr_surface *tl_surface(const struct tinywl_toplevel *tl) {
	return tl->xsurface ? tl->xsurface->surface : tl->xdg_toplevel->base->surface;
}

static struct wlr_box tl_geometry(const struct tinywl_toplevel *tl) {
	if (tl->xsurface) {
		return (struct wlr_box){0, 0, tl->xsurface->width, tl->xsurface->height};
	}
	return tl->xdg_toplevel->base->geometry;
}

/* True once the client has something to configure: xdg is initialized, X11 is mapped. */
static bool tl_ready(const struct tinywl_toplevel *tl) {
	if (tl->xsurface) {
		return tl->xsurface->surface && tl->xsurface->surface->mapped;
	}
	return tl->xdg_toplevel->base->initialized;
}

static void tl_set_size(struct tinywl_toplevel *tl, int w, int h) {
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

static void tl_set_maximized(struct tinywl_toplevel *tl, bool on) {
	if (tl->xsurface) {
		wlr_xwayland_surface_set_maximized(tl->xsurface, on, on);
	} else {
		wlr_xdg_toplevel_set_maximized(tl->xdg_toplevel, on);
	}
}

static void tl_set_fullscreen(struct tinywl_toplevel *tl, bool on) {
	if (tl->xsurface) {
		wlr_xwayland_surface_set_fullscreen(tl->xsurface, on);
	} else {
		wlr_xdg_toplevel_set_fullscreen(tl->xdg_toplevel, on);
	}
}

static struct wlr_surface *tl_surface_at(const struct tinywl_toplevel *tl, double x, double y, double *sx, double *sy) {
	if (tl->xsurface) {
		return tl->xsurface->surface ? wlr_surface_surface_at(tl->xsurface->surface, x, y, sx, sy) : NULL;
	}
	return wlr_xdg_surface_surface_at(tl->xdg_toplevel->base, x, y, sx, sy);
}

static pid_t tl_pid(const struct tinywl_toplevel *tl) {
	pid_t pid = 0;
	if (tl->xsurface) {
		return tl->xsurface->pid;
	}
	wl_client_get_credentials(wl_resource_get_client(tl->xdg_toplevel->base->resource), &pid, NULL, NULL);
	return pid;
}

/* The window that had keyboard focus, whichever kind it is, is told it no longer does. */
static void surface_deactivate(struct wlr_surface *surface) {
	struct wlr_xdg_toplevel *xdg = wlr_xdg_toplevel_try_from_wlr_surface(surface);
	struct wlr_xwayland_surface *x = wlr_xwayland_surface_try_from_wlr_surface(surface);
	if (xdg) {
		wlr_xdg_toplevel_set_activated(xdg, false);
	} else if (x) {
		wlr_xwayland_surface_activate(x, false);
	}
}

/* Fit the frame around the window's current geometry and color it by focus. */
static void update_border(struct tinywl_toplevel *tl) {
	struct wlr_box geo = tl_geometry(tl), *g = &geo;
	bool focused = tl_surface(tl) && tl->server->seat->keyboard_state.focused_surface == tl_surface(tl);
	int t = BORDER_W;
	double z = tl->server->zoom * (1 - OPEN_POPIN * tl->faded);
	int r = lround(WIN_RADIUS * z);
	int gx = lround(g->x * z), gy = lround(g->y * z);
	int gw = lround(g->width * z), gh = lround(g->height * z);
	wlr_scene_node_set_enabled(&tl->border->node, !tl->mode);
	wlr_scene_node_set_enabled(&tl->glow->node, !tl->mode);
	wlr_scene_rect_set_size(tl->border, gw + 2 * t, gh + 2 * t);
	wlr_scene_rect_set_corner_radius(tl->border, r + t);
	float a = fminf(1.0f, fmaxf(0.0f, 1.0f - tl->faded)), bc[4], gc[4]; /* colours are premultiplied, so alpha scales every channel */
	for (int i = 0; i < 4; i++) {
		bc[i] = (focused ? BORDER_FOCUSED : BORDER_IDLE)[i] * a;
		gc[i] = (focused ? GLOW_FOCUSED : GLOW_IDLE)[i] * a;
	}
	wlr_scene_rect_set_color(tl->border, bc);
	wlr_scene_node_set_position(&tl->border->node, gx - t, gy - t);
	int bloom = 2 * GLOW_SIGMA;
	wlr_scene_shadow_set_size(tl->glow, gw + 2 * t + 2 * bloom, gh + 2 * t + 2 * bloom);
	wlr_scene_shadow_set_corner_radius(tl->glow, r + t);
	wlr_scene_shadow_set_color(tl->glow, gc);
	wlr_scene_node_set_position(&tl->glow->node, gx - t - bloom, gy - t - bloom);
}

/* SEIZA stars. Territory size and grid spacing, in world units. New stars are
 * placed on a 3-column grid, left to right, then down. */
#define STAR_W 1920
#define STAR_H 1080
#define STAR_DX 2400
#define STAR_DY 1500
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

/* The first free slot of the 3-column grid. */
static int grid_slot(struct tinywl_server *server, double *cx, double *cy) {
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
static void star_new_name(struct tinywl_server *server, char name[32]) {
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
static struct star *star_create(struct tinywl_server *server) {
	double cx, cy;
	int slot = grid_slot(server, &cx, &cy);
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

static void persist_save(struct tinywl_server *server) {
	persist_write(server, server->constellation_name, true);
}

/* Build the stars of `c` and make the first one current. */
static void persist_apply(struct tinywl_server *server, const struct constellation *c) {
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
static void publish_state(struct tinywl_server *server) {
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
static void update_current_star(struct tinywl_server *server) {
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
#define MIN_WIN_W 200
#define MIN_WIN_H 120

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
};

static double mono_s(void);
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
	/* The unscaled size wlroots itself uses (surface.c: surface_reconfigure);
	 * xdg windows aren't clipped, so it is just the surface size. */
	int w = ss->surface->current.width, h = ss->surface->current.height;
	if (w <= 0 || h <= 0) {
		return;
	}
	wlr_scene_buffer_set_dest_size(sb, fmax(1, lround(w * z)), fmax(1, lround(h * z)));
	wlr_scene_buffer_set_corner_radius(sb, arg->radius);
	wlr_scene_buffer_set_opacity(sb, arg->opacity);
}

/* Put one window where world + camera + zoom say it should be. */
static void layout_toplevel(struct tinywl_toplevel *tl) {
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
	wlr_scene_node_set_position(&tl->scene_tree->node, lround(tl->wx * z + (z - pz) * geo.width / 2), lround(tl->wy * z + (z - pz) * geo.height / 2));
	if (tl->xsurface && surface && surface->mapped && tl->xsurface->width && tl->xsurface->height &&
			(tl->xsurface->x != 0 || tl->xsurface->y != 0)) {
		/* Every X11 window sits at X position (0,0), whatever its place on our canvas: X only delivers
		 * the pointer to a window inside its own screen (the size of the output), and the canvas is far
		 * bigger than that. Which window gets the pointer follows Wayland pointer focus, not X geometry. */
		wlr_xwayland_surface_configure(tl->xsurface, 0, 0, tl->xsurface->width, tl->xsurface->height);
	}
	struct scale_arg arg = {pz, tl->mode ? 0 : lround(WIN_RADIUS * pz), fminf(1.0f, fmaxf(0.0f, alpha))}; /* maximized windows have square corners */
	wlr_scene_node_for_each_buffer(&tl->scene_tree->node, scale_buffer, &arg);
	update_border(tl);
}

static void layout_unmanaged(struct xunmanaged *u) {
	double z = u->server->zoom;
	wlr_scene_node_set_position(&u->tree->node, lround((u->ax + u->xs->x) * z), lround((u->ay + u->xs->y) * z));
	struct scale_arg arg = {z, 0, 1.0f};
	wlr_scene_node_for_each_buffer(&u->tree->node, scale_buffer, &arg);
}

/* Re-place the canvas and every window; call after the camera or zoom changes. */
static void relayout_all(struct tinywl_server *server) {
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

static void anim_cancel(struct tinywl_server *server);

/* Multiply the zoom by `factor`, keeping the world point under the cursor
 * fixed on screen (so it zooms toward the pointer, like a map). */
static void zoom_at(struct tinywl_server *server, double factor) {
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

static double mono_s(void) {
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec + ts.tv_nsec / 1e9;
}

static void anim_cancel(struct tinywl_server *server) {
	server->anim.on = false;
}

/* One step of the glide, to the camera the clock says it should have now. It runs
 * from output_frame, so every frame shown has exactly one step in it: a separate
 * timer drifts against the screen's refresh and makes the glide stutter. */
static void anim_step(struct tinywl_server *server) {
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

static void frames_all(struct tinywl_server *server) {
	struct tinywl_output *o;
	wl_list_for_each(o, &server->outputs, link) {
		wlr_output_schedule_frame(o->wlr_output);
	}
}

/* One step of every window that is still fading in; true while any is. */
static bool open_step(struct tinywl_server *server) {
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
static void fly_to_star(struct tinywl_server *server, struct star *s) {
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
static struct star *star_dot_at(struct tinywl_server *server, double x, double y) {
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
static void overview_pointer(struct tinywl_server *server) {
	struct star *s = star_dot_at(server, server->cursor->x, server->cursor->y);
	if (s != server->hover_star) {
		server->hover_star = s;
		relayout_all(server); /* recolors the hovered star */
	}
}

static bool main_mod_held(struct tinywl_server *server) {
	struct wlr_keyboard *kb = wlr_seat_get_keyboard(server->seat);
	return kb && (wlr_keyboard_get_modifiers(kb) & WLR_MODIFIER_LOGO);
}

static void focus_toplevel(struct tinywl_toplevel *toplevel) {
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
	anim_cancel(server);
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

static void star_move(struct tinywl_server *server, struct star *s, double dx, double dy);

/* Lay the constellation's stars out on a figure, in order, making stars for the points that
 * have none, and join them with the figure's threads; "none" cuts every thread. The figure is
 * only a template: afterwards the stars are free to move and the threads are the constellation's own. */
static int op_set_figure(void *ud, const char *name) {
	struct tinywl_server *server = ud;
	struct star *s;
	if (strcmp(name, "none") == 0) {
		server->nlinks = 0;
	} else {
		struct figure f;
		if (figure_find(server->cfg_dir, name, &f) != 0) {
			return -1;
		}
		double scale = figure_scale(&f, STAR_DX, STAR_DY), x, y;
		int have = wl_list_length(&server->stars);
		if (f.n > CONST_STARS_MAX) {
			return -1;
		}
		for (int i = have; i < f.n; i++) { /* a star for every point that has none */
			char nm[32];
			star_new_name(server, nm);
			figure_point(&f, i, scale, STAR_W / 2.0, STAR_H / 2.0, &x, &y);
			star_create_at(server, nm, x, y);
		}
		int i = 0;
		wl_list_for_each(s, &server->stars, link) {
			if (i < f.n) {
				figure_point(&f, i, scale, STAR_W / 2.0, STAR_H / 2.0, &x, &y);
				star_move(server, s, x - s->cx, y - s->cy);
			}
			i++;
		}
		i = 0;
		wl_list_for_each(s, &server->stars, link) { /* stars past the last point go to free grid slots */
			if (i++ >= f.n) {
				int slot = grid_slot(server, &x, &y);
				(void)slot;
				star_move(server, s, x - s->cx, y - s->cy);
			}
		}
		server->nlinks = 0;
		for (int l = 0; l < f.nlinks && server->nlinks < CONST_LINKS_MAX; l++) {
			server->links[server->nlinks][0] = f.links[l][0];
			server->links[server->nlinks++][1] = f.links[l][1];
		}
	}
	persist_save(server);
	relayout_all(server);
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
	server->nlinks = 0;
}

/* Star n of the constellation, counting from 1 in creation order; 0 is the current one. */
static struct star *star_nth(struct tinywl_server *server, int n) {
	if (n == 0) {
		return server->current_star;
	}
	struct star *s;
	wl_list_for_each(s, &server->stars, link) {
		if (--n == 0) {
			return s;
		}
	}
	return NULL;
}

/* Only an empty star can go, and there is always at least one. */
static int op_delete_star(void *ud, int n) {
	struct tinywl_server *server = ud;
	struct star *s = star_nth(server, n);
	struct tinywl_toplevel *tl;
	if (!s || wl_list_length(&server->stars) <= 1) {
		return -1;
	}
	wl_list_for_each(tl, &server->all_toplevels, all_link) { /* unmapped windows count too */
		if (tl->star == s) {
			return -1;
		}
	}
	struct star *next = wl_container_of(s->link.next == &server->stars ?
		server->stars.next : s->link.next, next, link);
	bool was_current = s == server->current_star;
	int gone = 0; /* its place in the list: the threads to it go, and later stars move up one */
	for (struct star *t = s; t->link.prev != &server->stars; ) {
		t = wl_container_of(t->link.prev, t, link);
		gone++;
	}
	int kept = 0;
	for (int l = 0; l < server->nlinks; l++) {
		int a = server->links[l][0], b = server->links[l][1];
		if (a != gone && b != gone) {
			server->links[kept][0] = a > gone ? a - 1 : a;
			server->links[kept++][1] = b > gone ? b - 1 : b;
		}
	}
	server->nlinks = kept;
	wl_list_remove(&s->link);
	wlr_scene_node_destroy(&s->marker->node);
	free(s);
	server->hover_star = NULL;
	if (was_current) {
		fly_to_star(server, next);
	} else {
		relayout_all(server); /* the camera stays where it is */
	}
	persist_save(server);
	return 0;
}

/* A star's name is a line in the constellation file: printable, one line, and short. */
static int op_rename_star(void *ud, int n, const char *name) {
	struct tinywl_server *server = ud;
	struct star *s = star_nth(server, n);
	size_t len = strlen(name);
	if (!s || len == 0 || len >= sizeof(s->name)) {
		return -1;
	}
	for (size_t i = 0; i < len; i++) {
		if ((unsigned char)name[i] < 0x20 || name[i] == 0x7f) {
			return -1;
		}
	}
	snprintf(s->name, sizeof(s->name), "%s", name);
	persist_save(server);
	relayout_all(server);
	return 0;
}

/* True if a star other than `except` sits so near (x, y) that their territories would overlap. */
static bool star_too_near(struct tinywl_server *server, const struct star *except, double x, double y) {
	struct star *o;
	wl_list_for_each(o, &server->stars, link) {
		if (o != except && fabs(o->cx - x) < STAR_W && fabs(o->cy - y) < STAR_H) {
			return true;
		}
	}
	return false;
}

/* A star at a place of your choosing (the center of its territory), if there is room. */
static int op_add_star_at(void *ud, double x, double y) {
	struct tinywl_server *server = ud;
	if (wl_list_length(&server->stars) >= CONST_STARS_MAX || star_too_near(server, NULL, x, y)) {
		return -1;
	}
	char name[32];
	star_new_name(server, name);
	star_create_at(server, name, x, y);
	persist_save(server);
	relayout_all(server);
	return 0;
}

static int op_move_star(void *ud, int n, double x, double y) {
	struct tinywl_server *server = ud;
	struct star *s = star_nth(server, n);
	if (!s || star_too_near(server, s, x, y)) {
		return -1;
	}
	star_move(server, s, x - s->cx, y - s->cy);
	persist_save(server);
	return 0;
}

/* Say, in the log, what Seiza thinks every window is: for working out why a program misbehaves. */
static void op_debug_windows(void *ud) {
	struct tinywl_server *server = ud;
	struct tinywl_toplevel *tl;
	int i = 0;
	wlr_log(WLR_INFO, "seiza windows: zoom %.2f, camera (%.0f,%.0f), pointer focus %p, keyboard focus %p",
		server->zoom, server->cam_x, server->cam_y, (void *)server->seat->pointer_state.focused_surface, (void *)server->seat->keyboard_state.focused_surface);
	wl_list_for_each(tl, &server->all_toplevels, all_link) { /* mapped ones, and ones that exist but are not shown */
		struct wlr_box g = tl_geometry(tl);
		int star = 0;
		struct star *s;
		wl_list_for_each(s, &server->stars, link) {
			if (s == tl->star) {
				break;
			}
			star++;
		}
		wlr_log(WLR_INFO, "seiza windows: #%d %s %s star %d at (%.0f,%.0f) %dx%d (drawn %dx%d) mode %d surface %p class '%s' title '%s'", i++,
			tl->xsurface ? "x11" : "wayland", tl->scene_tree->node.enabled ? "shown" : "NOT SHOWN", star, tl->wx, tl->wy, g.width, g.height,
			tl->x_scene ? tl->x_scene->buffer->dst_width : -1, tl->x_scene ? tl->x_scene->buffer->dst_height : -1, tl->mode, (void *)tl_surface(tl),
			tl->xsurface && tl->xsurface->class ? tl->xsurface->class : (tl->xdg_toplevel && tl->xdg_toplevel->app_id ? tl->xdg_toplevel->app_id : ""),
			tl->xsurface && tl->xsurface->title ? tl->xsurface->title : (tl->xdg_toplevel && tl->xdg_toplevel->title ? tl->xdg_toplevel->title : ""));
	}
	struct xunmanaged *um;
	wl_list_for_each(um, &server->unmanaged, link) {
		wlr_log(WLR_INFO, "seiza windows: x11 menu/tooltip at (%d,%d) in window at (%.0f,%.0f) %dx%d %s class '%s'", um->xs->x, um->xs->y, um->ax, um->ay, um->xs->width, um->xs->height,
			um->surf ? "shown" : "hidden", um->xs->class ? um->xs->class : "");
	}
}

/* For tests on a machine with no mouse: put the pointer there and click, through the same code a
 * real pointer goes through. Off unless SEIZA_DEBUG_INPUT is set. */
static void server_cursor_button(struct wl_listener *listener, void *data);
static void process_cursor_motion(struct tinywl_server *server, uint32_t time);
static struct tinywl_toplevel *desktop_toplevel_at(struct tinywl_server *server, double lx, double ly, struct wlr_surface **surface, double *sx, double *sy);
static int op_debug_pointer(void *ud, double x, double y, int act) {
	struct tinywl_server *server = ud;
	if (!getenv("SEIZA_DEBUG_INPUT")) {
		return -1;
	}
	wlr_seat_set_capabilities(server->seat, WL_SEAT_CAPABILITY_POINTER | WL_SEAT_CAPABILITY_KEYBOARD);
	static uint32_t now_ms = 1000; /* real events carry rising times, and X programs drop ones that do not */
	now_ms += 50;
	wlr_cursor_warp(server->cursor, NULL, x, y);
	process_cursor_motion(server, now_ms);
	wlr_seat_pointer_notify_frame(server->seat); /* a real device ends each event with a frame */
	{
		double sx = 0, sy = 0;
		struct wlr_surface *surf = NULL;
		struct tinywl_toplevel *t = desktop_toplevel_at(server, server->cursor->x, server->cursor->y, &surf, &sx, &sy);
		wlr_log(WLR_INFO, "seiza debug: asked (%.0f,%.0f) cursor (%.1f,%.1f) cam (%.0f,%.0f) zoom %.2f -> window %p surface %p local (%.1f,%.1f)",
			x, y, server->cursor->x, server->cursor->y, server->cam_x, server->cam_y, server->zoom, (void *)t, (void *)surf, sx, sy);
	}
	if (act == 2) {
		zoom_at(server, 1 / 1.1); /* one wheel notch out, like Super+scroll */
		process_cursor_motion(server, now_ms += 50);
		wlr_seat_pointer_notify_frame(server->seat);
	}
	if (act == 1 || act == 3) { /* click, or just the press */
		struct wlr_pointer_button_event ev = {.time_msec = now_ms, .button = BTN_LEFT, .state = WL_POINTER_BUTTON_STATE_PRESSED};
		server_cursor_button(&server->cursor_button, &ev);
		wlr_seat_pointer_notify_frame(server->seat);
	}
	if (act == 1 || act == 4) { /* click, or just the release */
		struct wlr_pointer_button_event ev = {.time_msec = now_ms += 50, .button = BTN_LEFT, .state = WL_POINTER_BUTTON_STATE_RELEASED};
		server_cursor_button(&server->cursor_button, &ev);
		wlr_seat_pointer_notify_frame(server->seat);
	}
	return 0;
}

/* A thread between stars a and b (1-based): there is one afterwards if there was none, and not if there was. */
static int op_toggle_link(void *ud, int a, int b) {
	struct tinywl_server *server = ud;
	int count = wl_list_length(&server->stars);
	if (a < 1 || b < 1 || a > count || b > count || a == b) {
		return -1;
	}
	a--;
	b--;
	for (int l = 0; l < server->nlinks; l++) {
		if ((server->links[l][0] == a && server->links[l][1] == b) || (server->links[l][0] == b && server->links[l][1] == a)) {
			server->links[l][0] = server->links[server->nlinks - 1][0];
			server->links[l][1] = server->links[--server->nlinks][1];
			goto done;
		}
	}
	if (server->nlinks >= CONST_LINKS_MAX) {
		return -1;
	}
	server->links[server->nlinks][0] = a;
	server->links[server->nlinks++][1] = b;
done:
	persist_save(server);
	relayout_all(server);
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
	wl_list_for_each(tl, &server->all_toplevels, all_link) {
		tl->star = server->current_star;
		tl->wx = tl->star->cx - STAR_W / 2.0 + 60 + 40 * (n % 8);
		tl->wy = tl->star->cy - STAR_H / 2.0 + 60 + 40 * (n % 8);
		n++;
	}
	constellation_set_last(server->cfg_dir, name);
	fly_to_star(server, server->current_star);
	return 0;
}

/* A new constellation, empty but for one star, and open it. The one you were in is kept as it was. */
static int op_new_constellation(void *ud, const char *name) {
	struct tinywl_server *server = ud;
	struct constellation c;
	if (constellation_load(server->cfg_dir, name, &c) == 0) {
		return -1; /* never overwrite one that is there */
	}
	constellation_default(&c);
	snprintf(c.name, sizeof(c.name), "%s", name);
	if (constellation_save(&c, server->cfg_dir) != 0) {
		return -1;
	}
	return op_load(server, name);
}

/* One part of ~/.config/seiza/seiza.kdl ("motion", "frame", "keys", "autostart") in the old flat
 * file format, from `seiza-conf export`, which also logs mistakes in the file to stderr. NULL when
 * there is no seiza.kdl or it says nothing about that part. Parsed after the old file, so it wins. */
static char *cfg_kdl(const struct tinywl_server *server, const char *part) {
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

static void op_spawn(void *ud, const char *cmdline);
static struct wl_event_source *g_idle_timer;
static int64_t g_last_input_ms;
static bool g_dimmed, g_locked_by_idle, g_off; /* what the idle clock has done since the last input */

static int64_t now_ms(void) {
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

static void outputs_power(struct tinywl_server *server, bool on) {
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
static int g_night_force = -1;                /* -1 follow the schedule, 0 off, 1 on */
static double g_night_last = 6500;            /* the temperature the screens were last set to */
static struct wlr_color_transform *g_night_tr; /* NULL = colours untouched */
static struct wl_event_source *g_night_timer;

static void night_apply(struct tinywl_server *server) {
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

static int night_tick(void *data) {
	night_apply(data);
	wl_event_source_timer_update(g_night_timer, 30000); /* the ramp is 30 minutes long: twice a minute is plenty */
	return 0;
}

/* Any key, pointer move, button or scroll: the idle clock starts over, the dim lifts and sleeping screens wake. */
static void idle_activity(struct tinywl_server *server) {
	g_last_input_ms = now_ms();
	if (g_off) {
		outputs_power(server, true);
	}
	if (g_dimmed) {
		wlr_scene_node_set_enabled(&server->dim_rect->node, false);
	}
	g_dimmed = g_locked_by_idle = g_off = false;
}

static int idle_tick(void *data) {
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
static void frame_colors_load(struct tinywl_server *server) {
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
static void keys_dump(const struct keymap_cfg *k) {
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

static void op_spawn(void *ud, const char *cmdline);

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

static void op_spawn(void *ud, const char *cmdline);

/* Programs to start with the session, one command per line in ~/.config/seiza/autostart
 * ('#' comments). With no such file the satellites that ship with Seiza start;
 * an empty file starts nothing. Only run at startup, never on reload-config. */
static void autostart(struct tinywl_server *server) {
	static char text[8192] = "seiza-starfield\nseiza-bar\nseiza-widgets\nseiza-osd\nseiza-notify\nwl-paste --type text --watch seiza-clip-store\nseiza-conf slideshow\n";
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

static void rules_load(struct tinywl_server *server);
static void tile_load(struct tinywl_server *server);
static void tile_star(struct tinywl_server *server, struct star *s);
static int op_reload_config(void *ud) {
	struct tinywl_server *server = ud;
	keys_load(server);
	rules_load(server);
	tile_load(server);
	frame_colors_load(server);
	relayout_all(server); /* a new radius or border changes every window */
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

static int reload_timer_fired(void *ud) {
	wlr_log(WLR_INFO, "seiza: config changed, reloading");
	op_reload_config(ud);
	return 0;
}

static int config_changed(int fd, uint32_t mask, void *ud) {
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

static void watch_config(struct tinywl_server *server) {
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
static void toplevel_set_mode(struct tinywl_toplevel *tl, int mode) {
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

static void rules_load(struct tinywl_server *server) {
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
static void rules_apply(struct tinywl_toplevel *tl) {
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

static void tile_load(struct tinywl_server *server) {
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

static void tile_star(struct tinywl_server *server, struct star *s) {
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

static const struct cmd_ops seiza_ops = {
	op_overview, op_new_star, op_delete_star, op_save, op_load, op_new_constellation, op_relaunch,
	op_reload_config, op_quit, op_pan, op_zoom_reset, op_cycle_window, op_spawn,
	op_move_window, op_resize_window, op_close_window, op_goto_star, op_help, op_focus_dir,
	op_toggle_maximize, op_set_figure, op_rename_star, op_add_star_at, op_move_star, op_debug_windows, op_debug_pointer, op_toggle_link,
	op_switcher, op_focus_window, op_toggle_tile, op_night,
};

/* Run the binding for this key combo, if there is one. True if it was one. */
static bool handle_keybinding(struct tinywl_server *server, uint32_t mods, xkb_keysym_t sym) {
	/* Super+Escape (and Alt+Escape) always quit, even if the keys file is broken or rebinds them. */
	if ((mods == KMOD_SUPER || mods == KMOD_ALT) && xkb_keysym_to_lower(sym) == XKB_KEY_Escape) {
		op_quit(server);
		return true;
	}
	const struct binding *b = keys_find(&server->keys, mods, sym);
	if (!b || server->locked) { /* a locked session gets no compositor keys, only the lock client */
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
	idle_activity(server);
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

/* A program (a game, through XWayland) can lock the pointer: it then reads raw motion and the cursor stays put.
 * shortcut: only a lock holds the pointer, a confine is accepted but not enforced; upgrade if a game needs real confinement. */
static void constraint_destroyed(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, constraint_destroy);
	wl_list_remove(&server->constraint_destroy.link);
	server->constraint = NULL;
}

static void constraint_set(struct tinywl_server *server, struct wlr_pointer_constraint_v1 *c) {
	if (server->constraint == c) {
		return;
	}
	if (server->constraint) {
		wl_list_remove(&server->constraint_destroy.link);
		wlr_pointer_constraint_v1_send_deactivated(server->constraint);
	}
	server->constraint = c;
	if (c) {
		server->constraint_destroy.notify = constraint_destroyed;
		wl_signal_add(&c->events.destroy, &server->constraint_destroy);
		wlr_pointer_constraint_v1_send_activated(c);
	}
}

static void server_new_constraint(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, new_constraint);
	struct wlr_pointer_constraint_v1 *c = data;
	if (c->surface == server->seat->pointer_state.focused_surface) { /* asked for while already pointed at */
		constraint_set(server, c);
	}
}

static bool pointer_locked(const struct tinywl_server *server) {
	return server->constraint && server->constraint->type == WLR_POINTER_CONSTRAINT_V1_LOCKED
		&& server->cursor_mode == TINYWL_CURSOR_PASSTHROUGH;
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
	constraint_set(server, event->new_surface ? wlr_pointer_constraints_v1_constraint_for_surface(server->constraints, event->new_surface, server->seat) : NULL);
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

static void layer_focus(struct tinywl_server *server, struct wlr_surface *surface);

/* The layer-shell surface under a screen point, in the upper (above windows)
 * or lower layers. Layers are not zoomed, so scene coordinates are screen ones. */
static bool layer_surface_at(struct tinywl_server *server, double lx, double ly, bool upper,
		struct wlr_surface **surface, double *sx, double *sy) {
	for (int i = 0; i < 2; i++) {
		double nx, ny;
		struct wlr_scene_tree *t = server->layers[upper ? 3 - i : 1 - i];
		struct wlr_scene_node *n = wlr_scene_node_at(&t->node, lx, ly, &nx, &ny);
		if (n && n->type == WLR_SCENE_NODE_BUFFER) {
			struct wlr_scene_surface *ss = wlr_scene_surface_try_from_buffer(wlr_scene_buffer_from_node(n));
			if (ss) {
				*surface = ss->surface;
				*sx = nx;
				*sy = ny;
				return true;
			}
		}
	}
	return false;
}

static struct tinywl_toplevel *desktop_toplevel_at(
		struct tinywl_server *server, double lx, double ly,
		struct wlr_surface **surface, double *sx, double *sy) {
	/* SEIZA: wlr_scene_node_at tests clicks against the UNscaled surface, so it
	 * breaks when zoomed. Instead, convert the screen point to world coords and
	 * ask each window, front to back (the list is in stacking order), for the
	 * surface under that point. sx/sy come back in surface-local coords, which
	 * is exactly what clients expect, at any zoom. */
	if (layer_surface_at(server, lx, ly, true, surface, sx, sy)) {
		return NULL; /* a surface, but no window: callers check `surface` */
	}
	if (server->overview) {
		return NULL; /* the overview is look-and-pick only, windows get no input */
	}
	double wxc = (lx + server->cam_x) / server->zoom;
	double wyc = (ly + server->cam_y) / server->zoom;
	struct xunmanaged *um;
	wl_list_for_each_reverse(um, &server->unmanaged, link) { /* menus and tooltips: a surface, but no window */
		if (um->surf) {
			struct wlr_surface *s = wlr_surface_surface_at(um->xs->surface, wxc - um->ax - um->xs->x, wyc - um->ay - um->xs->y, sx, sy);
			if (s) {
				*surface = s;
				return NULL;
			}
		}
	}
	struct tinywl_toplevel *tl;
	wl_list_for_each(tl, &server->toplevels, link) {
		struct wlr_surface *s = tl_surface_at(tl, wxc - tl->wx, wyc - tl->wy, sx, sy);
		if (s) {
			*surface = s;
			return tl;
		}
	}
	layer_surface_at(server, lx, ly, false, surface, sx, sy);
	return NULL;
}

static void reset_cursor_mode(struct tinywl_server *server) {
	/* SEIZA: a window dropped inside another star's territory moves to that
	 * star (judged by its center); dropped in empty space it keeps its star. */
	struct tinywl_toplevel *tl = server->grabbed_toplevel;
	struct star *from = tl ? tl->star : NULL;
	if (server->cursor_mode == TINYWL_CURSOR_MOVE && tl) {
		struct wlr_box g = tl_geometry(tl);
		struct star *s = star_at(server, tl->wx + g.x + g.width / 2.0,
			tl->wy + g.y + g.height / 2.0);
		if (s) {
			tl->star = s;
		}
	}
	if (server->cursor_mode == TINYWL_CURSOR_PAN) {
		update_current_star(server);
	}
	if (tl && server->cursor_mode != TINYWL_CURSOR_PASSTHROUGH) { /* a tiled star puts a moved or resized window back in its tile */
		tile_star(server, from);
		if (tl->star != from) {
			tile_star(server, tl->star);
		}
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

	struct wlr_box geo = tl_geometry(toplevel), *geo_box = &geo;
	toplevel->wx = new_left - geo_box->x;
	toplevel->wy = new_top - geo_box->y;
	layout_toplevel(toplevel);

	int new_width = new_right - new_left;
	int new_height = new_bottom - new_top;
	tl_set_size(toplevel, new_width, new_height);
}

/* Move a star and the windows that belong to it by a world-space delta. */
static void star_move(struct tinywl_server *server, struct star *s, double dx, double dy) {
	s->cx += dx;
	s->cy += dy;
	struct tinywl_toplevel *tl;
	wl_list_for_each(tl, &server->all_toplevels, all_link) {
		if (tl->star == s) {
			tl->wx += dx;
			tl->wy += dy;
		}
	}
	relayout_all(server);
}

/* Hot corner: the pointer rests in a 4 px corner box for panel_dwell ms and seiza-panel opens. It fires once
 * per visit, so closing the panel with the pointer still in the corner does not reopen it. */
static struct wl_event_source *g_corner_timer;
static bool g_corner_fired;

static int corner_fired(void *data) {
	struct tinywl_server *server = data;
	g_corner_fired = true;
	op_spawn(server, "seiza-panel open");
	return 0;
}

/* The hot corner of whichever monitor the pointer is on, if that corner is on the outside of the layout:
 * a corner at the seam between two monitors is not a corner, or crossing over would open the panel. */
static bool in_corner(struct tinywl_server *server) {
	struct wlr_output *out = wlr_output_layout_output_at(server->output_layout, server->cursor->x, server->cursor->y);
	int c = server->panel_corner;
	if (!out || c <= 0) {
		return false;
	}
	struct wlr_box f = {0};
	wlr_output_layout_get_box(server->output_layout, out, &f);
	double x = server->cursor->x - f.x, y = server->cursor->y - f.y;
	bool right = c == 1 || c == 3, bottom = c == 3 || c == 4;
	if (!(right ? x >= f.width - 4 : x < 4) || !(bottom ? y >= f.height - 4 : y < 4)) {
		return false;
	}
	double ix = right ? f.x + f.width - 2 : f.x + 1, iy = bottom ? f.y + f.height - 2 : f.y + 1; /* a point inside the corner */
	double ox = right ? f.x + f.width + 1 : f.x - 1, oy = bottom ? f.y + f.height + 1 : f.y - 1;
	return !wlr_output_layout_output_at(server->output_layout, ox, iy) && !wlr_output_layout_output_at(server->output_layout, ix, oy);
}

static void corner_motion(struct tinywl_server *server) {
	if (!g_corner_timer) {
		g_corner_timer = wl_event_loop_add_timer(wl_display_get_event_loop(server->wl_display), corner_fired, server);
	}
	if (server->locked || !in_corner(server)) {
		g_corner_fired = false;
		wl_event_source_timer_update(g_corner_timer, 0);
	} else if (!g_corner_fired) {
		wl_event_source_timer_update(g_corner_timer, server->panel_dwell > 0 ? server->panel_dwell : 1);
	}
}

static void process_cursor_motion(struct tinywl_server *server, uint32_t time) {
	idle_activity(server);
	corner_motion(server);
	if (server->locked) { /* the pointer belongs to the lock surface */
		struct wlr_session_lock_surface_v1 *ls = NULL;
		if (server->lock && !wl_list_empty(&server->lock->surfaces)) {
			ls = wl_container_of(server->lock->surfaces.next, ls, link);
		}
		if (ls) {
			struct wlr_box b = {0};
			wlr_output_layout_get_box(server->output_layout, ls->output, &b);
			wlr_seat_pointer_notify_enter(server->seat, ls->surface, server->cursor->x - b.x, server->cursor->y - b.y);
			wlr_seat_pointer_notify_motion(server->seat, time, server->cursor->x - b.x, server->cursor->y - b.y);
		}
		return;
	}
	/* SEIZA: in the overview the pointer only picks stars. */
	if (server->overview) {
		wlr_cursor_set_xcursor(server->cursor, server->cursor_mgr, "default");
		if (server->drag_star) {
			if (!server->drag_moved && hypot(server->cursor->x - server->press_x, server->cursor->y - server->press_y) < 6) {
				return; /* still a click */
			}
			server->drag_moved = true;
			star_move(server, server->drag_star,
				(server->cursor->x - server->pan_last_x) / server->zoom,
				(server->cursor->y - server->pan_last_y) / server->zoom);
			server->pan_last_x = server->cursor->x;
			server->pan_last_y = server->cursor->y;
			return;
		}
		overview_pointer(server);
		return;
	}
	/* SEIZA: dragging the canvas: the world follows the cursor like a map. */
	if (server->cursor_mode == TINYWL_CURSOR_PAN) {
		anim_cancel(server);
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
	/* Raw motion for programs that read it (games), in the window's own pixels. */
	double z = server->zoom > 0 ? server->zoom : 1;
	wlr_relative_pointer_manager_v1_send_relative_motion(server->relative_pointer, server->seat, (uint64_t)event->time_msec * 1000,
			event->delta_x / z, event->delta_y / z, event->unaccel_dx / z, event->unaccel_dy / z);
	if (pointer_locked(server)) {
		return; /* the cursor stays where it is while a program holds the pointer */
	}
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
	idle_activity(server);

	if (server->locked) {
		wlr_seat_pointer_notify_button(server->seat, event->time_msec, event->button, event->state);
		return;
	}

	/* SEIZA: in the constellation view a click on a star's dot flies into it and a
	 * drag moves the star (and its windows). */
	if (server->overview) {
		if (event->state == WL_POINTER_BUTTON_STATE_RELEASED && server->drag_star) {
			struct star *d = server->drag_star, *o;
			server->drag_star = NULL;
			if (!server->drag_moved) {
				fly_to_star(server, d);
				return;
			}
			wl_list_for_each(o, &server->stars, link) {
				if (o != d && fabs(o->cx - d->cx) < STAR_W && fabs(o->cy - d->cy) < STAR_H) {
					star_move(server, d, server->drag_x0 - d->cx, server->drag_y0 - d->cy);
					return; /* dropped on another star's territory: put it back */
				}
			}
			persist_save(server);
		} else if (event->state == WL_POINTER_BUTTON_STATE_PRESSED) {
			struct star *s = star_dot_at(server, server->cursor->x, server->cursor->y);
			if (s) {
				server->drag_star = s;
				server->drag_moved = false;
				server->drag_x0 = s->cx;
				server->drag_y0 = s->cy;
				server->press_x = server->pan_last_x = server->cursor->x;
				server->press_y = server->pan_last_y = server->cursor->y;
			}
		}
		return;
	}

	/* SEIZA: Super + left-drag moves the window under the cursor, Super +
	 * right-drag resizes it from the nearest corner, even when the app draws
	 * no title bar. The click is not forwarded to the app. */
	struct wlr_keyboard *kb = wlr_seat_get_keyboard(server->seat);
	if (event->state == WL_POINTER_BUTTON_STATE_PRESSED &&
			(event->button == BTN_LEFT || event->button == BTN_RIGHT) &&
			kb && (wlr_keyboard_get_modifiers(kb) & WLR_MODIFIER_LOGO)) {
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
				struct wlr_box geometry = tl_geometry(toplevel), *geo = &geometry;
				double mid_x = (toplevel->wx + geo->x + geo->width / 2.0) * server->zoom - server->cam_x;
				double mid_y = (toplevel->wy + geo->y + geo->height / 2.0) * server->zoom - server->cam_y;
				uint32_t edges = (server->cursor->x < mid_x ? WLR_EDGE_LEFT : WLR_EDGE_RIGHT) |
					(server->cursor->y < mid_y ? WLR_EDGE_TOP : WLR_EDGE_BOTTOM);
				begin_interactive(toplevel, TINYWL_CURSOR_RESIZE, edges);
			}
			return;
		}
		/* Super-click on empty space: grab the canvas and drag it. */
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
		struct wlr_layer_surface_v1 *ls = surface ? wlr_layer_surface_v1_try_from_wlr_surface(surface) : NULL;
		if (ls && ls->current.keyboard_interactive != ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE) {
			layer_focus(server, surface);
		}
	}
}

static void server_cursor_axis(struct wl_listener *listener, void *data) {
	/* This event is forwarded by the cursor when a pointer emits an axis event,
	 * for example when you move the scroll wheel. */
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_axis);
	struct wlr_pointer_axis_event *event = data;
	idle_activity(server);

	/* SEIZA: Super + scroll zooms toward the cursor (scroll up = zoom in) and is
	 * not forwarded to the app. */
	if (!server->locked && main_mod_held(server) && event->orientation == WL_POINTER_AXIS_VERTICAL_SCROLL) {
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

/* SEIZA rounded screen: four small black masks, one per corner, drawn above
 * everything. Each is a CORNER_R x CORNER_R antialiased quarter-circle bitmap. */
#define CORNER_R 22

struct corner_buf {
	struct wlr_buffer base;
	uint32_t px[CORNER_R * CORNER_R];
};

static void corner_destroy(struct wlr_buffer *b) {
	struct corner_buf *cb = wl_container_of(b, cb, base);
	free(cb);
}

static bool corner_begin(struct wlr_buffer *b, uint32_t flags, void **data, uint32_t *format, size_t *stride) {
	struct corner_buf *cb = wl_container_of(b, cb, base);
	if (flags & WLR_BUFFER_DATA_PTR_ACCESS_WRITE) {
		return false;
	}
	*data = cb->px;
	*format = DRM_FORMAT_ARGB8888;
	*stride = CORNER_R * 4;
	return true;
}

static void corner_end(struct wlr_buffer *b) {}

static const struct wlr_buffer_impl corner_impl = {
	.destroy = corner_destroy,
	.begin_data_ptr_access = corner_begin,
	.end_data_ptr_access = corner_end,
};

static struct wlr_buffer *corner_make(bool flip_x, bool flip_y) {
	struct corner_buf *cb = calloc(1, sizeof(*cb));
	wlr_buffer_init(&cb->base, &corner_impl, CORNER_R, CORNER_R);
	for (int y = 0; y < CORNER_R; y++) {
		for (int x = 0; x < CORNER_R; x++) {
			int fx = flip_x ? CORNER_R - 1 - x : x, fy = flip_y ? CORNER_R - 1 - y : y;
			double d = hypot(fx + 0.5 - CORNER_R, fy + 0.5 - CORNER_R);
			double a = fmin(1.0, fmax(0.0, d - CORNER_R + 0.5)); /* black outside the circle */
			cb->px[y * CORNER_R + x] = (uint32_t)lround(a * 255.0) << 24; /* premultiplied black */
		}
	}
	return &cb->base;
}

/* Create the corners on first use, then (re)place them on this output's edges. */
static void place_corners(struct tinywl_output *output) {
	struct tinywl_server *server = output->server;
	struct wlr_box box = {0};
	wlr_output_layout_get_box(server->output_layout, output->wlr_output, &box);
	for (int i = 0; i < 4; i++) {
		if (!output->corner[i]) {
			struct wlr_buffer *b = corner_make(i & 1, i >> 1);
			output->corner[i] = wlr_scene_buffer_create(server->corner_tree, b);
			wlr_buffer_drop(b);
		}
		wlr_scene_node_set_position(&output->corner[i]->node,
			box.x + (i & 1 ? box.width - CORNER_R : 0), box.y + (i >> 1 ? box.height - CORNER_R : 0));
	}
}

static void output_frame(struct wl_listener *listener, void *data) {
	/* This function is called every time an output is ready to display a frame,
	 * generally at the output's refresh rate (e.g. 60Hz). */
	struct tinywl_output *output = wl_container_of(listener, output, frame);
	struct wlr_scene *scene = output->server->scene;

	struct wlr_scene_output *scene_output = wlr_scene_get_scene_output(
		scene, output->wlr_output);

	anim_step(output->server);
	bool opening = open_step(output->server);

	/* A fullscreen window on the star in view covers the bar (layer Top); the constellation view hides it anyway. */
	bool covered = false;
	struct tinywl_toplevel *fs;
	wl_list_for_each(fs, &output->server->toplevels, link) {
		struct wlr_surface *fs_surf = tl_surface(fs);
		covered |= fs->mode == 2 && fs->star == output->server->current_star && fs_surf && fs_surf->mapped;
	}
	wlr_scene_node_set_enabled(&output->server->layers[2]->node, !output->server->overview && !covered);

	/* Render the scene if needed and commit the output */
	struct wlr_scene_output_state_options night = {.color_transform = g_night_tr};
	wlr_scene_output_commit(scene_output, &night);
	if (output->server->anim.on || opening) {
		wlr_output_schedule_frame(output->wlr_output); /* the glide is not done: another frame */
	}

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
	place_corners(output);
}

static void output_destroy(struct wl_listener *listener, void *data) {
	struct tinywl_output *output = wl_container_of(listener, output, destroy);

	struct seiza_layer *sl, *tmp;
	wl_list_for_each_safe(sl, tmp, &output->server->layer_surfaces, link) {
		if (sl->surf->output == output->wlr_output) {
			wlr_layer_surface_v1_destroy(sl->surf); /* the client sees "closed" */
		}
	}
	for (int i = 0; i < 4; i++) {
		if (output->corner[i]) {
			wlr_scene_node_destroy(&output->corner[i]->node);
		}
	}
	wl_list_remove(&output->frame.link);
	wl_list_remove(&output->request_state.link);
	wl_list_remove(&output->destroy.link);
	wl_list_remove(&output->link);
	free(output);
}

static bool g_env_ready; /* WAYLAND_DISPLAY is set: programs started now can reach us */

/* A monitor other than the primary one: the starfield from autostart only covers the primary, so start another
 * for this one, told where it sits in the layout. It exits when the monitor goes. */
static void start_starfield(struct tinywl_server *server, struct tinywl_output *output) {
	if (output->link.prev == &server->outputs) {
		return;
	}
	for (const char *c = output->wlr_output->name; *c; c++) {
		if (!isalnum((unsigned char)*c) && *c != '-' && *c != '_') {
			return;
		}
	}
	struct wlr_box b = {0};
	wlr_output_layout_get_box(server->output_layout, output->wlr_output, &b);
	char cmd[200];
	snprintf(cmd, sizeof(cmd), "seiza-starfield --on %s --at %d,%d", output->wlr_output->name, b.x, b.y);
	op_spawn(server, cmd);
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

	wl_list_insert(server->outputs.prev, &output->link); /* appended: the first monitor stays the primary one */

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
	place_corners(output);
	if (g_env_ready) {
		start_starfield(server, output);
	}
}

/* Remember how this window's program was launched, on its star, so the star can
 * offer to relaunch it later. Closing the window does not forget it. */
static void remember_app(struct tinywl_toplevel *tl) {
	struct star *s = tl->star;
	pid_t pid = tl_pid(tl);
	char cmd[256];
	if (pid <= 0 || procinfo_cmdline(pid, cmd, sizeof(cmd)) != 0) {
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

	if (toplevel->xsurface && !toplevel->x_scene) {
		toplevel->x_scene = wlr_scene_surface_create(toplevel->scene_tree, toplevel->xsurface->surface);
		/* The scene puts the drawn size back to the unscaled one on every commit, so our commit
		 * listener (which re-applies the zoom) has to come after its own: add it again, last. */
		wl_list_remove(&toplevel->commit.link);
		wl_signal_add(&toplevel->xsurface->surface->events.commit, &toplevel->commit);
		layout_toplevel(toplevel);
	}
	wl_list_insert(&toplevel->server->toplevels, &toplevel->link);
	if (!toplevel->server->motion.reduce) {
		toplevel->open_t0 = mono_s();
		frames_all(toplevel->server);
	}
	layout_toplevel(toplevel); /* it is on screen from now on: show it, frame and all */
	if (toplevel->xdg_toplevel && toplevel->xdg_toplevel->requested.fullscreen) { /* asked for before it could be answered */
		toplevel_set_mode(toplevel, 2);
		wlr_xdg_toplevel_set_fullscreen(toplevel->xdg_toplevel, true);
	}
	rules_apply(toplevel);
	tile_star(toplevel->server, toplevel->star);
	remember_app(toplevel);

	focus_toplevel(toplevel);
	publish_state(toplevel->server);
}

static void xdg_toplevel_unmap(struct wl_listener *listener, void *data) {
	/* Called when the surface is unmapped, and should no longer be shown. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, unmap);

	/* Reset the cursor mode if the grabbed toplevel was unmapped. */
	if (toplevel == toplevel->server->grabbed_toplevel) {
		reset_cursor_mode(toplevel->server);
	}

	if (toplevel->x_scene) {
		wlr_scene_node_destroy(&toplevel->x_scene->buffer->node);
		toplevel->x_scene = NULL;
	}
	wl_list_remove(&toplevel->link);
	layout_toplevel(toplevel); /* ...and not any more: take the frame away with it */
	tile_star(toplevel->server, toplevel->star);
	publish_state(toplevel->server);
}

static void xdg_toplevel_commit(struct wl_listener *listener, void *data) {
	/* Called when a new surface state is committed. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, commit);

	if (!toplevel->xsurface && toplevel->xdg_toplevel->base->initial_commit) {
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

	wl_list_remove(&toplevel->all_link);
	if (!toplevel->xsurface || toplevel->x_attached) {
		wl_list_remove(&toplevel->map.link);
		wl_list_remove(&toplevel->unmap.link);
		wl_list_remove(&toplevel->commit.link);
	}
	if (toplevel->xsurface) {
		wl_list_remove(&toplevel->associate.link);
		wl_list_remove(&toplevel->dissociate.link);
		wl_list_remove(&toplevel->request_configure.link);
		wl_list_remove(&toplevel->request_activate.link);
	}
	wl_list_remove(&toplevel->destroy.link);
	wl_list_remove(&toplevel->request_move.link);
	wl_list_remove(&toplevel->request_resize.link);
	wl_list_remove(&toplevel->request_maximize.link);
	wl_list_remove(&toplevel->request_fullscreen.link);

	if (toplevel->xsurface) {
		/* The scene tree of a Wayland window goes with its surface; an X11 window's is ours,
		 * with its frame and glow in it, and would stay on screen after the window closed. */
		wlr_scene_node_destroy(&toplevel->scene_tree->node);
	}
	free(toplevel);
}

static void begin_interactive(struct tinywl_toplevel *toplevel,
		enum tinywl_cursor_mode mode, uint32_t edges) {
	/* This function sets up an interactive move or resize operation, where the
	 * compositor stops propagating pointer events to clients and instead
	 * consumes them itself, to move or resize windows. */
	struct tinywl_server *server = toplevel->server;

	if (toplevel->mode) { /* leave maximized in place; a move also gets the old size back */
		toplevel->mode = 0;
		tl_set_maximized(toplevel, false);
		tl_set_fullscreen(toplevel, false);
		if (mode == TINYWL_CURSOR_MOVE) {
			tl_set_size(toplevel, toplevel->saved_w, toplevel->saved_h);
		}
		layout_toplevel(toplevel);
	}

	server->grabbed_toplevel = toplevel;
	server->cursor_mode = mode;

	/* SEIZA: all grab math is in world coordinates. */
	double cur_wx = (server->cursor->x + server->cam_x) / server->zoom;
	double cur_wy = (server->cursor->y + server->cam_y) / server->zoom;

	if (mode == TINYWL_CURSOR_MOVE) {
		server->grab_x = cur_wx - toplevel->wx;
		server->grab_y = cur_wy - toplevel->wy;
	} else {
		struct wlr_box geometry = tl_geometry(toplevel), *geo_box = &geometry;

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
		toplevel_set_mode(toplevel, toplevel->xdg_toplevel->requested.maximized ? 1 : 0);
		wlr_xdg_surface_schedule_configure(toplevel->xdg_toplevel->base); /* a reply is due even if nothing changed */
	}
}

static void xdg_toplevel_request_fullscreen(
		struct wl_listener *listener, void *data) {
	/* Just as with request_maximize, we must send a configure here. */
	struct tinywl_toplevel *toplevel =
		wl_container_of(listener, toplevel, request_fullscreen);
	if (toplevel->xdg_toplevel->base->initialized) {
		/* Fullscreen covers the bar too (games, video): Super+m brings the window back. */
		bool on = toplevel->xdg_toplevel->requested.fullscreen;
		toplevel_set_mode(toplevel, on ? 2 : 0);
		wlr_xdg_toplevel_set_fullscreen(toplevel->xdg_toplevel, on);
		wlr_xdg_surface_schedule_configure(toplevel->xdg_toplevel->base);
	}
}

/* What every new window gets, whichever kind: a frame, a star to live in, and a place in it. */
static void toplevel_init_common(struct tinywl_server *server, struct tinywl_toplevel *toplevel) {
	toplevel->border = wlr_scene_rect_create(toplevel->scene_tree, 0, 0, BORDER_IDLE);
	toplevel->glow = wlr_scene_shadow_create(toplevel->scene_tree, 0, 0, WIN_RADIUS, GLOW_SIGMA, GLOW_IDLE);
	wlr_scene_node_lower_to_bottom(&toplevel->border->node);
	wlr_scene_node_lower_to_bottom(&toplevel->glow->node);

	/* SEIZA: a new window lands in the star you are currently in, cascaded from
	 * that star's top-left so windows don't open stacked exactly on top of each
	 * other. */
	toplevel->star = server->current_star;
	wl_list_insert(&server->all_toplevels, &toplevel->all_link);
	int n = 0;
	struct tinywl_toplevel *other;
	wl_list_for_each(other, &server->toplevels, link) {
		n += other->star == toplevel->star;
	}
	n %= 8;
	toplevel->wx = toplevel->star->cx - STAR_W / 2.0 + 60 + 40 * n;
	toplevel->wy = toplevel->star->cy - STAR_H / 2.0 + 60 + 40 * n;
	layout_toplevel(toplevel);
}

/* Seiza draws the only frame there is, so every client is told to leave its own title bar and buttons off.
 * wlroots only takes the mode once the surface has had its initial commit, so a decoration that comes earlier waits for it. */
struct deco_wait {
	struct wl_listener commit, destroy;
	struct wlr_xdg_toplevel_decoration_v1 *d;
};

static void deco_wait_free(struct deco_wait *w) {
	wl_list_remove(&w->commit.link);
	wl_list_remove(&w->destroy.link);
	free(w);
}

static void deco_commit(struct wl_listener *listener, void *data) {
	struct deco_wait *w = wl_container_of(listener, w, commit);
	if (!w->d->toplevel->base->initial_commit)
		return;
	wlr_xdg_toplevel_decoration_v1_set_mode(w->d, WLR_XDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE);
	deco_wait_free(w);
}

static void deco_destroy(struct wl_listener *listener, void *data) {
	deco_wait_free(wl_container_of(listener, (struct deco_wait *)0, destroy));
}

static void server_new_decoration(struct wl_listener *listener, void *data) {
	struct wlr_xdg_toplevel_decoration_v1 *d = data;
	if (d->toplevel->base->initialized) {
		wlr_xdg_toplevel_decoration_v1_set_mode(d, WLR_XDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE);
		return;
	}
	struct deco_wait *w = calloc(1, sizeof *w);
	if (!w)
		return;
	w->d = d;
	w->commit.notify = deco_commit;
	wl_signal_add(&d->toplevel->base->surface->events.commit, &w->commit);
	w->destroy.notify = deco_destroy;
	wl_signal_add(&d->events.destroy, &w->destroy);
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

	toplevel_init_common(server, toplevel);

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

/* ---- SEIZA X11 windows (XWayland) ---- */

/* The X11 window has a Wayland surface from association until dissociation; map, unmap and
 * commit are listened to on it only meanwhile. */
static void xwl_associate(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, associate);
	struct wlr_surface *surface = tl->xsurface->surface;
	tl->map.notify = xdg_toplevel_map;
	wl_signal_add(&surface->events.map, &tl->map);
	tl->unmap.notify = xdg_toplevel_unmap;
	wl_signal_add(&surface->events.unmap, &tl->unmap);
	tl->commit.notify = xdg_toplevel_commit;
	wl_signal_add(&surface->events.commit, &tl->commit);
	tl->x_attached = true;
}

static void xwl_dissociate(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, dissociate);
	if (tl->x_attached) {
		wl_list_remove(&tl->map.link);
		wl_list_remove(&tl->unmap.link);
		wl_list_remove(&tl->commit.link);
		tl->x_attached = false;
	}
}

/* The program asks for a size; the position is ours to give. */
static void xwl_request_configure(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, request_configure);
	struct wlr_xwayland_surface_configure_event *e = data;
	wlr_xwayland_surface_configure(tl->xsurface, 0, 0, e->width, e->height);
}

static void xwl_request_move(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, request_move);
	begin_interactive(tl, TINYWL_CURSOR_MOVE, 0);
}

static void xwl_request_resize(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, request_resize);
	struct wlr_xwayland_resize_event *e = data;
	begin_interactive(tl, TINYWL_CURSOR_RESIZE, e->edges);
}

static void xwl_request_maximize(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, request_maximize);
	toplevel_set_mode(tl, tl->xsurface->maximized_horz && tl->xsurface->maximized_vert ? 1 : 0);
}

static void xwl_request_activate(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, request_activate);
	if (tl_ready(tl) && tl->star == tl->server->current_star) { /* a program in another star does not pull you there */
		focus_toplevel(tl);
	}
}

static void xwl_request_fullscreen(struct wl_listener *listener, void *data) {
	/* Fullscreen covers the bar, as for Wayland windows. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, request_fullscreen);
	toplevel_set_mode(toplevel, toplevel->xsurface->fullscreen ? 2 : 0);
}

static void server_new_xwayland_toplevel(struct tinywl_server *server, struct wlr_xwayland_surface *xs) {
	struct tinywl_toplevel *toplevel = calloc(1, sizeof(*toplevel));
	toplevel->server = server;
	toplevel->xsurface = xs;
	toplevel->scene_tree = wlr_scene_tree_create(server->canvas);
	toplevel->scene_tree->node.data = toplevel;
	xs->data = toplevel;
	toplevel_init_common(server, toplevel);

	toplevel->associate.notify = xwl_associate;
	wl_signal_add(&xs->events.associate, &toplevel->associate);
	toplevel->dissociate.notify = xwl_dissociate;
	wl_signal_add(&xs->events.dissociate, &toplevel->dissociate);
	toplevel->destroy.notify = xdg_toplevel_destroy;
	wl_signal_add(&xs->events.destroy, &toplevel->destroy);
	toplevel->request_configure.notify = xwl_request_configure;
	wl_signal_add(&xs->events.request_configure, &toplevel->request_configure);
	toplevel->request_move.notify = xwl_request_move;
	wl_signal_add(&xs->events.request_move, &toplevel->request_move);
	toplevel->request_resize.notify = xwl_request_resize;
	wl_signal_add(&xs->events.request_resize, &toplevel->request_resize);
	toplevel->request_maximize.notify = xwl_request_maximize;
	wl_signal_add(&xs->events.request_maximize, &toplevel->request_maximize);
	toplevel->request_fullscreen.notify = xwl_request_fullscreen;
	wl_signal_add(&xs->events.request_fullscreen, &toplevel->request_fullscreen);
	toplevel->request_activate.notify = xwl_request_activate;
	wl_signal_add(&xs->events.request_activate, &toplevel->request_activate);
	if (xs->surface) { /* it was associated before we heard of it */
		xwl_associate(&toplevel->associate, NULL);
	}
}

/* ...and the override-redirect ones: menus and tooltips, drawn where their program puts them. */
/* The scene resets the drawn size on every commit: put the zoom back, after it. */
static void um_commit(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, commit);
	layout_unmanaged(u);
}

static void um_map(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, map);
	u->surf = wlr_scene_surface_create(u->tree, u->xs->surface);
	/* It belongs to the window under the pointer (a tooltip, a menu just clicked), else the focused one. */
	struct tinywl_toplevel *owner = NULL, *t;
	struct wlr_surface *under = u->server->seat->pointer_state.focused_surface;
	wl_list_for_each(t, &u->server->toplevels, link) {
		if (under && tl_surface(t) == under) {
			owner = t;
			break;
		}
		if (!owner) {
			owner = t; /* the list starts with the focused window */
		}
	}
	u->ax = owner ? owner->wx : 0;
	u->ay = owner ? owner->wy : 0;
	u->commit.notify = um_commit;
	wl_signal_add(&u->xs->surface->events.commit, &u->commit); /* after the scene's own */
	u->committing = true;
	wlr_scene_node_raise_to_top(&u->tree->node);
	layout_unmanaged(u);
}

static void um_unmap(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, unmap);
	if (u->committing) {
		wl_list_remove(&u->commit.link);
		u->committing = false;
	}
	if (u->surf) {
		wlr_scene_node_destroy(&u->surf->buffer->node);
		u->surf = NULL;
	}
}

static void um_associate(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, associate);
	u->map.notify = um_map;
	wl_signal_add(&u->xs->surface->events.map, &u->map);
	u->unmap.notify = um_unmap;
	wl_signal_add(&u->xs->surface->events.unmap, &u->unmap);
	u->attached = true;
}

static void um_dissociate(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, dissociate);
	if (u->attached) {
		wl_list_remove(&u->map.link);
		wl_list_remove(&u->unmap.link);
		u->attached = false;
	}
}

static void um_moved(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, set_geometry);
	layout_unmanaged(u);
}

static void um_request_configure(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, request_configure);
	struct wlr_xwayland_surface_configure_event *e = data;
	wlr_xwayland_surface_configure(u->xs, e->x, e->y, e->width, e->height); /* it knows where it wants to be */
	layout_unmanaged(u);
}

static void um_destroy(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, destroy);
	if (u->committing) {
		wl_list_remove(&u->commit.link);
	}
	if (u->attached) {
		wl_list_remove(&u->map.link);
		wl_list_remove(&u->unmap.link);
	}
	wl_list_remove(&u->associate.link);
	wl_list_remove(&u->dissociate.link);
	wl_list_remove(&u->destroy.link);
	wl_list_remove(&u->set_geometry.link);
	wl_list_remove(&u->request_configure.link);
	wl_list_remove(&u->link);
	wlr_scene_node_destroy(&u->tree->node);
	free(u);
}

static void server_new_xwayland_unmanaged(struct tinywl_server *server, struct wlr_xwayland_surface *xs) {
	struct xunmanaged *u = calloc(1, sizeof(*u));
	u->server = server;
	u->xs = xs;
	u->tree = wlr_scene_tree_create(server->xtop);
	wl_list_insert(server->unmanaged.prev, &u->link);
	u->associate.notify = um_associate;
	wl_signal_add(&xs->events.associate, &u->associate);
	u->dissociate.notify = um_dissociate;
	wl_signal_add(&xs->events.dissociate, &u->dissociate);
	u->destroy.notify = um_destroy;
	wl_signal_add(&xs->events.destroy, &u->destroy);
	u->set_geometry.notify = um_moved;
	wl_signal_add(&xs->events.set_geometry, &u->set_geometry);
	u->request_configure.notify = um_request_configure;
	wl_signal_add(&xs->events.request_configure, &u->request_configure);
	if (xs->surface) {
		um_associate(&u->associate, NULL);
	}
}

static void server_new_xwayland_surface(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, new_xwayland_surface);
	struct wlr_xwayland_surface *xs = data;
	if (xs->override_redirect) {
		server_new_xwayland_unmanaged(server, xs);
	} else {
		server_new_xwayland_toplevel(server, xs);
	}
}

/* Xwayland is up: it shares our seat (keyboard, clipboard) and shows our pointer over its windows. */
static void xwayland_ready(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, xwayland_ready);
	wlr_xwayland_set_seat(server->xwayland, server->seat);
	struct wlr_xcursor *xc = wlr_xcursor_manager_get_xcursor(server->cursor_mgr, "default", 1);
	if (xc && xc->image_count > 0) {
		struct wlr_xcursor_image *img = xc->images[0];
		wlr_xwayland_set_cursor(server->xwayland, wlr_xcursor_image_get_buffer(img), img->hotspot_x, img->hotspot_y);
	}
	wlr_log(WLR_INFO, "seiza: xwayland ready on DISPLAY=%s", server->xwayland->display_name);
}

/* Keyboard focus to a layer surface (launcher, menu), deactivating the window that had it. */
static void layer_focus(struct tinywl_server *server, struct wlr_surface *surface) {
	if (server->locked) {
		return;
	}
	struct wlr_surface *prev = server->seat->keyboard_state.focused_surface;
	if (prev) {
		surface_deactivate(prev);
	}
	struct wlr_keyboard *kb = wlr_seat_get_keyboard(server->seat);
	if (kb) {
		wlr_seat_keyboard_notify_enter(server->seat, surface, kb->keycodes, kb->num_keycodes, &kb->modifiers);
	}
}

/* Place every layer surface against its output, top layer first so that
 * exclusive zones stack the way clients expect.
 * shortcut: the usable area they leave over is ignored, windows float on the
 * canvas under bars; upgrade if windows should avoid bars. */
static void layers_arrange(struct tinywl_server *server) {
	struct tinywl_output *o;
	wl_list_for_each(o, &server->outputs, link) {
		struct wlr_box full = {0}, usable;
		wlr_output_layout_get_box(server->output_layout, o->wlr_output, &full);
		usable = full;
		for (int layer = 3; layer >= 0; layer--) {
			struct seiza_layer *sl;
			wl_list_for_each(sl, &server->layer_surfaces, link) {
				if (sl->surf->output == o->wlr_output && (int)sl->surf->current.layer == layer) {
					wlr_scene_layer_surface_v1_configure(sl->scene, &full, &usable);
				}
			}
		}
		if (o->link.prev == &server->outputs) { /* only the primary monitor sets what windows keep clear of */
			server->full = full;
			server->usable = usable;
		}
	}
}

static void layer_commit(struct wl_listener *listener, void *data) {
	struct seiza_layer *sl = wl_container_of(listener, sl, commit);
	if (!sl->surf->output) {
		return;
	}
	wlr_scene_node_reparent(&sl->scene->tree->node, sl->server->layers[sl->surf->current.layer]);
	layers_arrange(sl->server);
}

static void layer_map(struct wl_listener *listener, void *data) {
	struct seiza_layer *sl = wl_container_of(listener, sl, map);
	struct wlr_layer_surface_v1 *ls = sl->surf;
	if (ls->current.layer >= ZWLR_LAYER_SHELL_V1_LAYER_TOP &&
			ls->current.keyboard_interactive != ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE) {
		layer_focus(sl->server, ls->surface);
	}
}

static void layer_unmap(struct wl_listener *listener, void *data) {
	struct seiza_layer *sl = wl_container_of(listener, sl, unmap);
	struct tinywl_server *server = sl->server;
	if (server->seat->keyboard_state.focused_surface == sl->surf->surface &&
			!wl_list_empty(&server->toplevels)) {
		focus_toplevel(wl_container_of(server->toplevels.next, (struct tinywl_toplevel *)NULL, link));
	}
}

static void layer_destroy(struct wl_listener *listener, void *data) {
	struct seiza_layer *sl = wl_container_of(listener, sl, destroy);
	wl_list_remove(&sl->commit.link);
	wl_list_remove(&sl->map.link);
	wl_list_remove(&sl->unmap.link);
	wl_list_remove(&sl->destroy.link);
	wl_list_remove(&sl->link);
	free(sl);
}

static void server_new_layer_surface(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, new_layer_surface);
	struct wlr_layer_surface_v1 *ls = data;
	if (!ls->output) { /* the client let us pick */
		if (wl_list_empty(&server->outputs)) {
			wlr_layer_surface_v1_destroy(ls);
			return;
		}
		struct tinywl_output *o = wl_container_of(server->outputs.next, o, link); /* the primary monitor... */
		const char *on = ls->namespace ? strchr(ls->namespace, '@') : NULL; /* "seiza-starfield@HDMI-A-1": asks for that monitor */
		if (on) {
			struct tinywl_output *w;
			wl_list_for_each(w, &server->outputs, link) {
				if (!strcmp(w->wlr_output->name, on + 1)) {
					o = w;
				}
			}
		}
		bool fixed = on || (ls->namespace && (!strcmp(ls->namespace, "seiza-bar") || !strcmp(ls->namespace, "seiza-starfield") || !strcmp(ls->namespace, "seiza-widgets")));
		struct wlr_output *at = fixed ? NULL : wlr_output_layout_output_at(server->output_layout, server->cursor->x, server->cursor->y);
		ls->output = at ? at : o->wlr_output; /* ...but pop-ups (panel, launcher, menus) open where the pointer is */
	}
	struct seiza_layer *sl = calloc(1, sizeof(*sl));
	sl->server = server;
	sl->surf = ls;
	sl->scene = wlr_scene_layer_surface_v1_create(server->layers[ls->pending.layer], ls);
	ls->data = sl->scene->tree; /* popups of this surface attach here */
	wl_list_insert(&server->layer_surfaces, &sl->link);
	sl->commit.notify = layer_commit;
	wl_signal_add(&ls->surface->events.commit, &sl->commit);
	sl->map.notify = layer_map;
	wl_signal_add(&ls->surface->events.map, &sl->map);
	sl->unmap.notify = layer_unmap;
	wl_signal_add(&ls->surface->events.unmap, &sl->unmap);
	sl->destroy.notify = layer_destroy;
	wl_signal_add(&ls->events.destroy, &sl->destroy);
}

/* ---- SEIZA session lock ---- */

static void lock_new_surface(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, lock_new_surface);
	struct wlr_session_lock_surface_v1 *ls = data;
	struct wlr_box b = {0};
	wlr_output_layout_get_box(server->output_layout, ls->output, &b);
	struct wlr_scene_surface *ss = wlr_scene_surface_create(server->lock_tree, ls->surface);
	wlr_scene_node_set_position(&ss->buffer->node, b.x, b.y);
	wlr_session_lock_surface_v1_configure(ls, b.width, b.height);
	struct wlr_keyboard *kb = wlr_seat_get_keyboard(server->seat);
	if (kb) {
		wlr_seat_keyboard_notify_enter(server->seat, ls->surface, kb->keycodes, kb->num_keycodes, &kb->modifiers);
	}
}

static void lock_unlock(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, lock_unlock);
	server->locked = false;
	if (server->lock_bg) {
		wlr_scene_node_destroy(&server->lock_bg->node);
		server->lock_bg = NULL;
	}
	wlr_seat_keyboard_notify_clear_focus(server->seat);
	if (!wl_list_empty(&server->toplevels)) {
		focus_toplevel(wl_container_of(server->toplevels.next, (struct tinywl_toplevel *)NULL, link));
	}
	wlr_log(WLR_INFO, "seiza: unlocked");
}

/* The lock client went away. If it did not unlock first the session stays locked
 * (black) until a new lock client takes over, as the protocol requires. */
static void lock_destroy(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, lock_destroy);
	wl_list_remove(&server->lock_new_surface.link);
	wl_list_remove(&server->lock_unlock.link);
	wl_list_remove(&server->lock_destroy.link);
	server->lock = NULL;
}

static void server_new_lock(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, new_lock);
	struct wlr_session_lock_v1 *lock = data;
	if (server->lock) { /* already locked by a live client */
		wlr_session_lock_v1_destroy(lock);
		return;
	}
	server->lock = lock;
	server->locked = true;
	if (!server->lock_bg) {
		struct wlr_box b = {0};
		wlr_output_layout_get_box(server->output_layout, NULL, &b);
		float black[4] = {0, 0, 0, 1};
		server->lock_bg = wlr_scene_rect_create(server->lock_tree, b.width, b.height, black);
		wlr_scene_node_set_position(&server->lock_bg->node, b.x, b.y);
	}
	wlr_seat_keyboard_notify_clear_focus(server->seat);
	wlr_seat_pointer_clear_focus(server->seat);
	server->lock_new_surface.notify = lock_new_surface;
	wl_signal_add(&lock->events.new_surface, &server->lock_new_surface);
	server->lock_unlock.notify = lock_unlock;
	wl_signal_add(&lock->events.unlock, &server->lock_unlock);
	server->lock_destroy.notify = lock_destroy;
	wl_signal_add(&lock->events.destroy, &server->lock_destroy);
	wlr_session_lock_v1_send_locked(lock); /* the black rect already covers every output */
	wlr_log(WLR_INFO, "seiza: locked");
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
	struct wlr_layer_surface_v1 *lparent = wlr_layer_surface_v1_try_from_wlr_surface(xdg_popup->parent);
	assert(parent != NULL || lparent != NULL); /* popups of layer surfaces (menus, tooltips) hang off the layer's tree */
	struct wlr_scene_tree *parent_tree = parent ? parent->data : lparent->data;
	xdg_popup->base->data = wlr_scene_xdg_surface_create(parent_tree, xdg_popup->base);

	popup->commit.notify = xdg_popup_commit;
	wl_signal_add(&xdg_popup->base->surface->events.commit, &popup->commit);

	popup->destroy.notify = xdg_popup_destroy;
	wl_signal_add(&xdg_popup->events.destroy, &popup->destroy);
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
