/* SEIZA: outputs and the rounded screen corners. */
#include "seiza.h"

/* SEIZA rounded screen: four small black masks, one per corner, drawn above
 * everything. Each is an r x r antialiased quarter-circle bitmap; r is FRAME.screen (0 = square screen). */
struct corner_buf {
	struct wlr_buffer base;
	int r;
	uint32_t px[];
};

void corner_destroy(struct wlr_buffer *b) {
	struct corner_buf *cb = wl_container_of(b, cb, base);
	free(cb);
}

bool corner_begin(struct wlr_buffer *b, uint32_t flags, void **data, uint32_t *format, size_t *stride) {
	struct corner_buf *cb = wl_container_of(b, cb, base);
	if (flags & WLR_BUFFER_DATA_PTR_ACCESS_WRITE) {
		return false;
	}
	*data = cb->px;
	*format = DRM_FORMAT_ARGB8888;
	*stride = cb->r * 4;
	return true;
}

void corner_end(struct wlr_buffer *b) {}

static const struct wlr_buffer_impl corner_impl = {
	.destroy = corner_destroy,
	.begin_data_ptr_access = corner_begin,
	.end_data_ptr_access = corner_end,
};

struct wlr_buffer *corner_make(int r, bool flip_x, bool flip_y) {
	struct corner_buf *cb = calloc(1, sizeof(*cb) + sizeof(uint32_t) * r * r);
	cb->r = r;
	wlr_buffer_init(&cb->base, &corner_impl, r, r);
	for (int y = 0; y < r; y++) {
		for (int x = 0; x < r; x++) {
			int fx = flip_x ? r - 1 - x : x, fy = flip_y ? r - 1 - y : y;
			double d = hypot(fx + 0.5 - r, fy + 0.5 - r);
			double a = fmin(1.0, fmax(0.0, d - r + 0.5)); /* black outside the circle */
			cb->px[y * r + x] = (uint32_t)lround(a * 255.0) << 24; /* premultiplied black */
		}
	}
	return &cb->base;
}

/* Create the corners on first use, then (re)place them on this output's edges. */
void place_corners(struct tinywl_output *output) {
	struct tinywl_server *server = output->server;
	struct wlr_box box = {0};
	wlr_output_layout_get_box(server->output_layout, output->wlr_output, &box);
	int r = FRAME.screen;
	for (int i = 0; i < 4; i++) {
		if (output->corner[i] && output->corner_r != r) { /* the radius changed: rebuild */
			wlr_scene_node_destroy(&output->corner[i]->node);
			output->corner[i] = NULL;
		}
		if (!r) {
			continue;
		}
		if (!output->corner[i]) {
			struct wlr_buffer *b = corner_make(r, i & 1, i >> 1);
			output->corner[i] = wlr_scene_buffer_create(server->corner_tree, b);
			wlr_buffer_drop(b);
		}
		wlr_scene_node_set_position(&output->corner[i]->node,
			box.x + (i & 1 ? box.width - r : 0), box.y + (i >> 1 ? box.height - r : 0));
	}
	output->corner_r = r;
}

void output_frame(struct wl_listener *listener, void *data) {
	/* This function is called every time an output is ready to display a frame,
	 * generally at the output's refresh rate (e.g. 60Hz). */
	struct tinywl_output *output = wl_container_of(listener, output, frame);
	struct wlr_scene *scene = output->server->scene;

	struct wlr_scene_output *scene_output = wlr_scene_get_scene_output(
		scene, output->wlr_output);

	anim_step(output->server);
	bool opening = open_step(output->server);
	opening |= ghost_step(output->server);

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

void output_request_state(struct wl_listener *listener, void *data) {
	/* This function is called when the backend requests a new state for
	 * the output. For example, Wayland and X11 backends request a new mode
	 * when the output window is resized. */
	struct tinywl_output *output = wl_container_of(listener, output, request_state);
	const struct wlr_output_event_request_state *event = data;
	wlr_output_commit_state(output->wlr_output, event->state);
	place_corners(output);
}

static bool g_lost_all_outputs; /* the last monitor went away, taking the satellites' layer surfaces with it */
void output_destroy(struct wl_listener *listener, void *data) {
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
	g_lost_all_outputs = wl_list_empty(&output->server->outputs);
	free(output);
}

bool g_env_ready; /* WAYLAND_DISPLAY is set: programs started now can reach us */

/* A monitor other than the primary one: the starfield from autostart only covers the primary, so start another
 * for this one, told where it sits in the layout. It exits when the monitor goes. */
void start_starfield(struct tinywl_server *server, struct tinywl_output *output) {
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

void server_new_output(struct wl_listener *listener, void *data) {
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
	if (g_lost_all_outputs && g_env_ready) { /* e.g. after suspend the panel reappears: the satellites exited when it went */
		g_lost_all_outputs = false;
		/* shortcut: a fixed list, not the user's autostart (which also holds programs that survive), when it needs to follow a custom autostart */
		for (const char *sat[] = {"seiza-starfield", "seiza-bar", "seiza-widgets", "seiza-osd", "seiza-notify", NULL}, **c = sat; *c; c++) {
			op_spawn(server, *c);
		}
	}
}
