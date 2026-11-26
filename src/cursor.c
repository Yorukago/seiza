/* SEIZA: the pointer: motion, buttons, drags, hot corner, constraints. */
#include "seiza.h"

/* The layer-shell surface under a screen point, in the upper (above windows)
 * or lower layers. Layers are not zoomed, so scene coordinates are screen ones. */
bool layer_surface_at(struct tinywl_server *server, double lx, double ly, bool upper,
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

struct tinywl_toplevel *desktop_toplevel_at(
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

void reset_cursor_mode(struct tinywl_server *server) {
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

void process_cursor_move(struct tinywl_server *server) {
	/* Move the grabbed toplevel to the new position. */
	struct tinywl_toplevel *toplevel = server->grabbed_toplevel;
	toplevel->wx = (server->cursor->x + server->cam_x) / server->zoom - server->grab_x;
	toplevel->wy = (server->cursor->y + server->cam_y) / server->zoom - server->grab_y;
	layout_toplevel(toplevel);
}

void process_cursor_resize(struct tinywl_server *server) {
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
void star_move(struct tinywl_server *server, struct star *s, double dx, double dy) {
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

int corner_fired(void *data) {
	struct tinywl_server *server = data;
	g_corner_fired = true;
	op_spawn(server, "seiza-panel open");
	return 0;
}

/* The hot corner of whichever monitor the pointer is on, if that corner is on the outside of the layout:
 * a corner at the seam between two monitors is not a corner, or crossing over would open the panel. */
bool in_corner(struct tinywl_server *server) {
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

void corner_motion(struct tinywl_server *server) {
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

void process_cursor_motion(struct tinywl_server *server, uint32_t time) {
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

void server_cursor_motion(struct wl_listener *listener, void *data) {
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

void server_cursor_motion_absolute(
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

void server_cursor_button(struct wl_listener *listener, void *data) {
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
	/* SEIZA: a press focuses the window first and only then reaches it: Electron apps (Vesktop) drop a
	 * click that arrives before their window is active, so they needed two. */
	if (event->state == WL_POINTER_BUTTON_STATE_PRESSED) {
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
	/* Notify the client with pointer focus that a button event has occurred */
	wlr_seat_pointer_notify_button(server->seat,
			event->time_msec, event->button, event->state);
	if (event->state == WL_POINTER_BUTTON_STATE_RELEASED) {
		/* If you released any buttons, we exit interactive move/resize mode. */
		reset_cursor_mode(server);
	}
}

void server_cursor_axis(struct wl_listener *listener, void *data) {
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

void server_cursor_frame(struct wl_listener *listener, void *data) {
	/* This event is forwarded by the cursor when a pointer emits an frame
	 * event. Frame events are sent after regular pointer events to group
	 * multiple events together. For instance, two axis events may happen at the
	 * same time, in which case a frame event won't be sent in between. */
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_frame);
	/* Notify the client with pointer focus of the frame event. */
	wlr_seat_pointer_notify_frame(server->seat);
}
