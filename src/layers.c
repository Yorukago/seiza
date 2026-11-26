/* SEIZA: layer-shell surfaces: bars, launchers, wallpapers. */
#include "seiza.h"

/* Keyboard focus to a layer surface (launcher, menu), deactivating the window that had it. */
void layer_focus(struct tinywl_server *server, struct wlr_surface *surface) {
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
void layers_arrange(struct tinywl_server *server) {
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

void layer_commit(struct wl_listener *listener, void *data) {
	struct seiza_layer *sl = wl_container_of(listener, sl, commit);
	if (!sl->surf->output) {
		return;
	}
	wlr_scene_node_reparent(&sl->scene->tree->node, sl->server->layers[sl->surf->current.layer]);
	layers_arrange(sl->server);
}

void layer_map(struct wl_listener *listener, void *data) {
	struct seiza_layer *sl = wl_container_of(listener, sl, map);
	struct wlr_layer_surface_v1 *ls = sl->surf;
	if (ls->current.layer >= ZWLR_LAYER_SHELL_V1_LAYER_TOP &&
			ls->current.keyboard_interactive != ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE) {
		layer_focus(sl->server, ls->surface);
	}
}

void layer_unmap(struct wl_listener *listener, void *data) {
	struct seiza_layer *sl = wl_container_of(listener, sl, unmap);
	struct tinywl_server *server = sl->server;
	if (server->seat->keyboard_state.focused_surface == sl->surf->surface &&
			!wl_list_empty(&server->toplevels)) {
		focus_toplevel(wl_container_of(server->toplevels.next, (struct tinywl_toplevel *)NULL, link));
	}
}

void layer_destroy(struct wl_listener *listener, void *data) {
	struct seiza_layer *sl = wl_container_of(listener, sl, destroy);
	wl_list_remove(&sl->commit.link);
	wl_list_remove(&sl->map.link);
	wl_list_remove(&sl->unmap.link);
	wl_list_remove(&sl->destroy.link);
	wl_list_remove(&sl->link);
	free(sl);
}

void server_new_layer_surface(struct wl_listener *listener, void *data) {
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
