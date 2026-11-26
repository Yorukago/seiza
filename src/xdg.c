/* SEIZA: xdg windows: map/unmap, decorations, popups, interactive move and resize. */
#include "seiza.h"

/* Remember how this window's program was launched, on its star, so the star can
 * offer to relaunch it later. Closing the window does not forget it. */
void remember_app(struct tinywl_toplevel *tl) {
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

void xdg_toplevel_map(struct wl_listener *listener, void *data) {
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

void xdg_toplevel_unmap(struct wl_listener *listener, void *data) {
	/* Called when the surface is unmapped, and should no longer be shown. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, unmap);

	/* Reset the cursor mode if the grabbed toplevel was unmapped. */
	if (toplevel == toplevel->server->grabbed_toplevel) {
		reset_cursor_mode(toplevel->server);
	}

	ghost_start(toplevel);
	if (toplevel->x_scene) {
		wlr_scene_node_destroy(&toplevel->x_scene->buffer->node);
		toplevel->x_scene = NULL;
	}
	wl_list_remove(&toplevel->link);
	layout_toplevel(toplevel); /* ...and not any more: take the frame away with it */
	tile_star(toplevel->server, toplevel->star);
	publish_state(toplevel->server);
}

void xdg_toplevel_commit(struct wl_listener *listener, void *data) {
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

void xdg_toplevel_destroy(struct wl_listener *listener, void *data) {
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

void begin_interactive(struct tinywl_toplevel *toplevel,
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

void xdg_toplevel_request_move(
		struct wl_listener *listener, void *data) {
	/* This event is raised when a client would like to begin an interactive
	 * move, typically because the user clicked on their client-side
	 * decorations. Note that a more sophisticated compositor should check the
	 * provided serial against a list of button press serials sent to this
	 * client, to prevent the client from requesting this whenever they want. */
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, request_move);
	begin_interactive(toplevel, TINYWL_CURSOR_MOVE, 0);
}

void xdg_toplevel_request_resize(
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

void xdg_toplevel_request_maximize(
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

void xdg_toplevel_request_fullscreen(
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
void toplevel_init_common(struct tinywl_server *server, struct tinywl_toplevel *toplevel) {
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

void server_new_decoration(struct wl_listener *listener, void *data) {
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

void server_new_xdg_toplevel(struct wl_listener *listener, void *data) {
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

/* ---- SEIZA session lock ---- */

void xdg_popup_commit(struct wl_listener *listener, void *data) {
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

void xdg_popup_destroy(struct wl_listener *listener, void *data) {
	/* Called when the xdg_popup is destroyed. */
	struct tinywl_popup *popup = wl_container_of(listener, popup, destroy);

	wl_list_remove(&popup->commit.link);
	wl_list_remove(&popup->destroy.link);

	free(popup);
}

void server_new_xdg_popup(struct wl_listener *listener, void *data) {
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
