/* SEIZA: ext-session-lock: the lock screen. */
#include "seiza.h"

void lock_new_surface(struct wl_listener *listener, void *data) {
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

void lock_unlock(struct wl_listener *listener, void *data) {
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
void lock_destroy(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, lock_destroy);
	wl_list_remove(&server->lock_new_surface.link);
	wl_list_remove(&server->lock_unlock.link);
	wl_list_remove(&server->lock_destroy.link);
	server->lock = NULL;
}

void server_new_lock(struct wl_listener *listener, void *data) {
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
