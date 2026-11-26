/* SEIZA: keyboards, new input devices, key bindings, seat requests. */
#include "seiza.h"

/* Run the binding for this key combo, if there is one. True if it was one. */
bool handle_keybinding(struct tinywl_server *server, uint32_t mods, xkb_keysym_t sym) {
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

void keyboard_handle_key(
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

void keyboard_handle_destroy(struct wl_listener *listener, void *data) {
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

void server_new_keyboard(struct tinywl_server *server,
		struct wlr_input_device *device) {
	struct wlr_keyboard *wlr_keyboard = wlr_keyboard_from_input_device(device);

	struct tinywl_keyboard *keyboard = calloc(1, sizeof(*keyboard));
	keyboard->server = server;
	keyboard->wlr_keyboard = wlr_keyboard;

	const char *layout = getenv("XKB_DEFAULT_LAYOUT");
	keyboard_set_layout(wlr_keyboard, layout ? layout : "pt");
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

void server_new_pointer(struct tinywl_server *server,
		struct wlr_input_device *device) {
	/* We don't do anything special with pointers. All of our pointer handling
	 * is proxied through wlr_cursor. On another compositor, you might take this
	 * opportunity to do libinput configuration on the device to set
	 * acceleration, etc. */
	/* SEIZA: libinput leaves tap-to-click off, so a laptop touchpad would not click when tapped. */
	if (wlr_input_device_is_libinput(device)) {
		struct libinput_device *li = wlr_libinput_get_device_handle(device);
		if (libinput_device_config_tap_get_finger_count(li) > 0) {
			libinput_device_config_tap_set_enabled(li, LIBINPUT_CONFIG_TAP_ENABLED);
		}
	}
	wlr_cursor_attach_input_device(server->cursor, device);
}

void server_new_input(struct wl_listener *listener, void *data) {
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

void seat_request_cursor(struct wl_listener *listener, void *data) {
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
void constraint_destroyed(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, constraint_destroy);
	wl_list_remove(&server->constraint_destroy.link);
	server->constraint = NULL;
}

void constraint_set(struct tinywl_server *server, struct wlr_pointer_constraint_v1 *c) {
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

void server_new_constraint(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, new_constraint);
	struct wlr_pointer_constraint_v1 *c = data;
	if (c->surface == server->seat->pointer_state.focused_surface) { /* asked for while already pointed at */
		constraint_set(server, c);
	}
}

bool pointer_locked(const struct tinywl_server *server) {
	return server->constraint && server->constraint->type == WLR_POINTER_CONSTRAINT_V1_LOCKED
		&& server->cursor_mode == TINYWL_CURSOR_PASSTHROUGH;
}

void seat_pointer_focus_change(struct wl_listener *listener, void *data) {
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

void seat_request_set_selection(struct wl_listener *listener, void *data) {
	/* This event is raised by the seat when a client wants to set the selection,
	 * usually when the user copies something. wlroots allows compositors to
	 * ignore such requests if they so choose, but in tinywl we always honor
	 */
	struct tinywl_server *server = wl_container_of(
			listener, server, request_set_selection);
	struct wlr_seat_request_set_selection_event *event = data;
	wlr_seat_set_selection(server->seat, event->source, event->serial);
}
