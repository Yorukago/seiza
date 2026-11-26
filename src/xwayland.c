/* SEIZA: X11 windows through XWayland. */
#include "seiza.h"

/* The X11 window has a Wayland surface from association until dissociation; map, unmap and
 * commit are listened to on it only meanwhile. */
void xwl_associate(struct wl_listener *listener, void *data) {
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

void xwl_dissociate(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, dissociate);
	if (tl->x_attached) {
		wl_list_remove(&tl->map.link);
		wl_list_remove(&tl->unmap.link);
		wl_list_remove(&tl->commit.link);
		tl->x_attached = false;
	}
}

/* The program asks for a size; the position is ours to give. */
void xwl_request_configure(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, request_configure);
	struct wlr_xwayland_surface_configure_event *e = data;
	wlr_xwayland_surface_configure(tl->xsurface, 0, 0, e->width, e->height);
}

void xwl_request_move(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, request_move);
	begin_interactive(tl, TINYWL_CURSOR_MOVE, 0);
}

void xwl_request_resize(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, request_resize);
	struct wlr_xwayland_resize_event *e = data;
	begin_interactive(tl, TINYWL_CURSOR_RESIZE, e->edges);
}

void xwl_request_maximize(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, request_maximize);
	toplevel_set_mode(tl, tl->xsurface->maximized_horz && tl->xsurface->maximized_vert ? 1 : 0);
}

void xwl_request_activate(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *tl = wl_container_of(listener, tl, request_activate);
	if (tl_ready(tl) && tl->star == tl->server->current_star) { /* a program in another star does not pull you there */
		focus_toplevel(tl);
	}
}

void xwl_request_fullscreen(struct wl_listener *listener, void *data) {
	/* Fullscreen covers the bar, as for Wayland windows. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, request_fullscreen);
	toplevel_set_mode(toplevel, toplevel->xsurface->fullscreen ? 2 : 0);
}

void server_new_xwayland_toplevel(struct tinywl_server *server, struct wlr_xwayland_surface *xs) {
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
void um_commit(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, commit);
	layout_unmanaged(u);
}

void um_map(struct wl_listener *listener, void *data) {
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

void um_unmap(struct wl_listener *listener, void *data) {
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

void um_associate(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, associate);
	u->map.notify = um_map;
	wl_signal_add(&u->xs->surface->events.map, &u->map);
	u->unmap.notify = um_unmap;
	wl_signal_add(&u->xs->surface->events.unmap, &u->unmap);
	u->attached = true;
}

void um_dissociate(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, dissociate);
	if (u->attached) {
		wl_list_remove(&u->map.link);
		wl_list_remove(&u->unmap.link);
		u->attached = false;
	}
}

void um_moved(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, set_geometry);
	layout_unmanaged(u);
}

void um_request_configure(struct wl_listener *listener, void *data) {
	struct xunmanaged *u = wl_container_of(listener, u, request_configure);
	struct wlr_xwayland_surface_configure_event *e = data;
	wlr_xwayland_surface_configure(u->xs, e->x, e->y, e->width, e->height); /* it knows where it wants to be */
	layout_unmanaged(u);
}

void um_destroy(struct wl_listener *listener, void *data) {
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

void server_new_xwayland_unmanaged(struct tinywl_server *server, struct wlr_xwayland_surface *xs) {
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

void server_new_xwayland_surface(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, new_xwayland_surface);
	struct wlr_xwayland_surface *xs = data;
	if (xs->override_redirect) {
		server_new_xwayland_unmanaged(server, xs);
	} else {
		server_new_xwayland_toplevel(server, xs);
	}
}

/* Xwayland is up: it shares our seat (keyboard, clipboard) and shows our pointer over its windows. */
void xwayland_ready(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, xwayland_ready);
	wlr_xwayland_set_seat(server->xwayland, server->seat);
	struct wlr_xcursor *xc = wlr_xcursor_manager_get_xcursor(server->cursor_mgr, "default", 1);
	if (xc && xc->image_count > 0) {
		struct wlr_xcursor_image *img = xc->images[0];
		wlr_xwayland_set_cursor(server->xwayland, wlr_xcursor_image_get_buffer(img), img->hotspot_x, img->hotspot_y);
	}
	wlr_log(WLR_INFO, "seiza: xwayland ready on DISPLAY=%s", server->xwayland->display_name);
}
