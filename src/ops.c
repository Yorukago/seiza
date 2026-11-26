/* SEIZA: the operations behind every command (keys, socket, CLI). */
#include "seiza.h"

/* SEIZA: the operations behind every command (keys, socket, CLI). cmd.c does
 * the parsing; these do the work. `ud` is always the server. */
void op_overview(void *ud) {
	struct tinywl_server *server = ud;
	if (server->overview) {
		fly_to_star(server, server->current_star);
	} else {
		enter_overview(server);
	}
}

int op_new_star(void *ud) {
	struct tinywl_server *server = ud;
	fly_to_star(server, star_create(server));
	persist_save(server);
	return 0;
}


/* Lay the constellation's stars out on a figure, in order, making stars for the points that
 * have none, and join them with the figure's threads; "none" cuts every thread. The figure is
 * only a template: afterwards the stars are free to move and the threads are the constellation's own. */
int op_set_figure(void *ud, const char *name) {
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
struct star *star_nth(struct tinywl_server *server, int n) {
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
int op_delete_star(void *ud, int n) {
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
int op_rename_star(void *ud, int n, const char *name) {
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
bool star_too_near(struct tinywl_server *server, const struct star *except, double x, double y) {
	struct star *o;
	wl_list_for_each(o, &server->stars, link) {
		if (o != except && fabs(o->cx - x) < STAR_W && fabs(o->cy - y) < STAR_H) {
			return true;
		}
	}
	return false;
}

/* A star at a place of your choosing (the center of its territory), if there is room. */
int op_add_star_at(void *ud, double x, double y) {
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

int op_move_star(void *ud, int n, double x, double y) {
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
void op_debug_windows(void *ud) {
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
int op_debug_pointer(void *ud, double x, double y, int act) {
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
int op_toggle_link(void *ud, int a, int b) {
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
int op_save(void *ud, const char *name) {
	return persist_write(ud, name, false);
}

/* Replace the active constellation with `name`. Windows move to its first star. */
int op_load(void *ud, const char *name) {
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
int op_new_constellation(void *ud, const char *name) {
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
