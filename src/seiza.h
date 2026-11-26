/* SEIZA: the compositor's shared types. */
#ifndef SEIZA_H
#define SEIZA_H
#include <assert.h>
#include <getopt.h>
#include <stdbool.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <ctype.h>
#include <stdio.h>
#include <sys/wait.h>
#include <signal.h>
#include <time.h>
#include <unistd.h>
#include <wayland-server-core.h>
#include <wlr/backend.h>
#include <wlr/backend/libinput.h>
#include <libinput.h>
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
	struct wl_list ghosts;     /* closed windows still fading out */
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
	int corner_r; /* radius the corners were built with */
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
#define STAR_W 1920
#define STAR_H 1080

#define STAR_DX 2400
#define STAR_DY 1500

void fly_to_star(struct tinywl_server *server, struct star *s);
void enter_overview(struct tinywl_server *server);
struct star *star_create(struct tinywl_server *server);
void persist_save(struct tinywl_server *server);
void star_new_name(struct tinywl_server *server, char name[32]);
struct star *star_create_at(struct tinywl_server *server, const char *name,
		double cx, double cy);
int grid_slot(struct tinywl_server *server, double *cx, double *cy);
void relayout_all(struct tinywl_server *server);
struct wlr_box tl_geometry(const struct tinywl_toplevel *tl);
struct wlr_surface *tl_surface(const struct tinywl_toplevel *tl);
void zoom_at(struct tinywl_server *server, double factor);
int persist_write(struct tinywl_server *server, const char *name, bool make_last);
void persist_apply(struct tinywl_server *server, const struct constellation *c);
void star_move(struct tinywl_server *server, struct star *s, double dx, double dy);
void server_cursor_button(struct wl_listener *listener, void *data);
void process_cursor_motion(struct tinywl_server *server, uint32_t time);
struct tinywl_toplevel *desktop_toplevel_at(
		struct tinywl_server *server, double lx, double ly,
		struct wlr_surface **surface, double *sx, double *sy);
void op_overview(void *ud);
int op_new_star(void *ud);
int op_set_figure(void *ud, const char *name);
struct star *star_nth(struct tinywl_server *server, int n);
int op_delete_star(void *ud, int n);
int op_rename_star(void *ud, int n, const char *name);
bool star_too_near(struct tinywl_server *server, const struct star *except, double x, double y);
int op_add_star_at(void *ud, double x, double y);
int op_move_star(void *ud, int n, double x, double y);
void op_debug_windows(void *ud);
int op_debug_pointer(void *ud, double x, double y, int act);
int op_toggle_link(void *ud, int a, int b);
int op_save(void *ud, const char *name);
int op_load(void *ud, const char *name);
int op_new_constellation(void *ud, const char *name);

void xwl_associate(struct wl_listener *listener, void *data);
void xwl_dissociate(struct wl_listener *listener, void *data);
void xwl_request_configure(struct wl_listener *listener, void *data);
void xwl_request_move(struct wl_listener *listener, void *data);
void xwl_request_resize(struct wl_listener *listener, void *data);
void xwl_request_maximize(struct wl_listener *listener, void *data);
void xwl_request_activate(struct wl_listener *listener, void *data);
void xwl_request_fullscreen(struct wl_listener *listener, void *data);
void server_new_xwayland_toplevel(struct tinywl_server *server, struct wlr_xwayland_surface *xs);
void um_commit(struct wl_listener *listener, void *data);
void um_map(struct wl_listener *listener, void *data);
void um_unmap(struct wl_listener *listener, void *data);
void um_associate(struct wl_listener *listener, void *data);
void um_dissociate(struct wl_listener *listener, void *data);
void um_moved(struct wl_listener *listener, void *data);
void um_request_configure(struct wl_listener *listener, void *data);
void um_destroy(struct wl_listener *listener, void *data);
void server_new_xwayland_unmanaged(struct tinywl_server *server, struct wlr_xwayland_surface *xs);
void server_new_xwayland_surface(struct wl_listener *listener, void *data);
void xwayland_ready(struct wl_listener *listener, void *data);









void focus_toplevel(struct tinywl_toplevel *toplevel);











void toplevel_init_common(struct tinywl_server *server, struct tinywl_toplevel *toplevel);
void xdg_toplevel_destroy(struct wl_listener *listener, void *data);
void xdg_toplevel_unmap(struct wl_listener *listener, void *data);
void xdg_toplevel_commit(struct wl_listener *listener, void *data);
void toplevel_set_mode(struct tinywl_toplevel *tl, int mode);
bool tl_ready(const struct tinywl_toplevel *tl);
void begin_interactive(struct tinywl_toplevel *toplevel,
		enum tinywl_cursor_mode mode, uint32_t edges);
void layout_unmanaged(struct xunmanaged *u);
void xdg_toplevel_map(struct wl_listener *listener, void *data);

void layer_focus(struct tinywl_server *server, struct wlr_surface *surface);
void layers_arrange(struct tinywl_server *server);
void layer_commit(struct wl_listener *listener, void *data);
void layer_map(struct wl_listener *listener, void *data);
void layer_unmap(struct wl_listener *listener, void *data);
void layer_destroy(struct wl_listener *listener, void *data);
void server_new_layer_surface(struct wl_listener *listener, void *data);
void lock_new_surface(struct wl_listener *listener, void *data);
void lock_unlock(struct wl_listener *listener, void *data);
void lock_destroy(struct wl_listener *listener, void *data);
void server_new_lock(struct wl_listener *listener, void *data);

void surface_deactivate(struct wlr_surface *surface);

char *cfg_kdl(const struct tinywl_server *server, const char *part);
void outputs_power(struct tinywl_server *server, bool on);
void night_apply(struct tinywl_server *server);
int night_tick(void *data);
void idle_activity(struct tinywl_server *server);
int idle_tick(void *data);
void frame_colors_load(struct tinywl_server *server);
void keys_dump(const struct keymap_cfg *k);
void keys_load(struct tinywl_server *server);
void autostart(struct tinywl_server *server);
int op_reload_config(void *ud);
int reload_timer_fired(void *ud);
int config_changed(int fd, uint32_t mask, void *ud);
void watch_config(struct tinywl_server *server);

void tile_star(struct tinywl_server *server, struct star *s);
void frames_all(struct tinywl_server *server);
void update_border(struct tinywl_toplevel *tl);











extern struct wl_event_source *g_idle_timer;
extern int64_t g_last_input_ms;
extern int g_night_force;
extern struct wlr_color_transform *g_night_tr;
extern struct wl_event_source *g_night_timer;
extern struct frame FRAME;
extern float BORDER_FOCUSED[4], BORDER_IDLE[4], GLOW_FOCUSED[4];
#define WIN_RADIUS FRAME.radius
#define BORDER_W FRAME.border

void place_corners(struct tinywl_output *output);
void op_spawn(void *ud, const char *cmdline);
void rules_load(struct tinywl_server *server);
void tile_load(struct tinywl_server *server);











int64_t now_ms(void);

void remember_app(struct tinywl_toplevel *tl);
void xdg_toplevel_request_move(
		struct wl_listener *listener, void *data);
void xdg_toplevel_request_resize(
		struct wl_listener *listener, void *data);
void xdg_toplevel_request_maximize(
		struct wl_listener *listener, void *data);
void xdg_toplevel_request_fullscreen(
		struct wl_listener *listener, void *data);
void server_new_decoration(struct wl_listener *listener, void *data);
void server_new_xdg_toplevel(struct wl_listener *listener, void *data);
void xdg_popup_commit(struct wl_listener *listener, void *data);
void xdg_popup_destroy(struct wl_listener *listener, void *data);
void server_new_xdg_popup(struct wl_listener *listener, void *data);

double mono_s(void);
void rules_apply(struct tinywl_toplevel *tl);
pid_t tl_pid(const struct tinywl_toplevel *tl);
void ghost_start(struct tinywl_toplevel *tl);
void publish_state(struct tinywl_server *server);
void reset_cursor_mode(struct tinywl_server *server);
void tl_set_fullscreen(struct tinywl_toplevel *tl, bool on);
void tl_set_size(struct tinywl_toplevel *tl, int w, int h);
void layout_toplevel(struct tinywl_toplevel *tl);
void tl_set_maximized(struct tinywl_toplevel *tl, bool on);

void corner_destroy(struct wlr_buffer *b);
bool corner_begin(struct wlr_buffer *b, uint32_t flags, void **data, uint32_t *format, size_t *stride);
void corner_end(struct wlr_buffer *b);
struct wlr_buffer *corner_make(int r, bool flip_x, bool flip_y);
void output_frame(struct wl_listener *listener, void *data);
void output_request_state(struct wl_listener *listener, void *data);
void output_destroy(struct wl_listener *listener, void *data);
void start_starfield(struct tinywl_server *server, struct tinywl_output *output);
void server_new_output(struct wl_listener *listener, void *data);

bool open_step(struct tinywl_server *server);
void anim_step(struct tinywl_server *server);
bool ghost_step(struct tinywl_server *server);











#define GLOW_SIGMA 18

#define OPEN_POPIN 0.2 /* Hyprland's `popin 80%`: a window opens at 80% size and grows to 100% along its fade */

static const float GLOW_IDLE[4]      = {0, 0, 0, 0};

extern bool g_env_ready;

bool layer_surface_at(struct tinywl_server *server, double lx, double ly, bool upper,
		struct wlr_surface **surface, double *sx, double *sy);
void process_cursor_move(struct tinywl_server *server);
void process_cursor_resize(struct tinywl_server *server);
int corner_fired(void *data);
bool in_corner(struct tinywl_server *server);
void corner_motion(struct tinywl_server *server);
void server_cursor_motion(struct wl_listener *listener, void *data);
void server_cursor_motion_absolute(
		struct wl_listener *listener, void *data);
void server_cursor_axis(struct wl_listener *listener, void *data);
void server_cursor_frame(struct wl_listener *listener, void *data);

void anim_cancel(struct tinywl_server *server);
bool pointer_locked(const struct tinywl_server *server);
struct star *star_at(struct tinywl_server *server, double wx, double wy);
bool main_mod_held(struct tinywl_server *server);
void update_current_star(struct tinywl_server *server);
void overview_pointer(struct tinywl_server *server);
struct wlr_surface *tl_surface_at(const struct tinywl_toplevel *tl, double x, double y, double *sx, double *sy);
struct star *star_dot_at(struct tinywl_server *server, double x, double y);

#define MIN_WIN_W 200
#define MIN_WIN_H 120

bool handle_keybinding(struct tinywl_server *server, uint32_t mods, xkb_keysym_t sym);
void keyboard_handle_key(
		struct wl_listener *listener, void *data);
void keyboard_handle_destroy(struct wl_listener *listener, void *data);
void server_new_keyboard(struct tinywl_server *server,
		struct wlr_input_device *device);
void server_new_pointer(struct tinywl_server *server,
		struct wlr_input_device *device);
void server_new_input(struct wl_listener *listener, void *data);
void seat_request_cursor(struct wl_listener *listener, void *data);
void constraint_destroyed(struct wl_listener *listener, void *data);
void constraint_set(struct tinywl_server *server, struct wlr_pointer_constraint_v1 *c);
void server_new_constraint(struct wl_listener *listener, void *data);
void seat_pointer_focus_change(struct wl_listener *listener, void *data);
void seat_request_set_selection(struct wl_listener *listener, void *data);

int keyboard_set_layout(struct wlr_keyboard *kb, const char *layout);
void keyboard_handle_modifiers(
		struct wl_listener *listener, void *data);
void op_quit(void *ud);











extern const struct cmd_ops seiza_ops;

#endif
