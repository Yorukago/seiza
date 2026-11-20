/* SEIZA: key bindings. The `keys` file maps "Mods+Key" to a command line that
 * cmd_run() understands (a `seiza ...` action, or any shell command). The
 * shipped defaults are written in the same syntax, so there is one mechanism
 * and the user's file simply overrides them.
 *
 *     Super+Shift+Left = seiza pan -100 0
 *     Super+t          = kitty
 *     shell          = /usr/bin/ysh        # optional: shell used for non-seiza commands
 *
 * Key names are xkbcommon names for whatever the active layout produces. */
#ifndef SEIZA_KEYS_H
#define SEIZA_KEYS_H

#include <stdint.h>
#include <xkbcommon/xkbcommon.h>

/* Our own modifier bits, so this file does not need wlroots. */
#define KMOD_SHIFT 1
#define KMOD_CTRL  2
#define KMOD_ALT   4
#define KMOD_SUPER 8

#define KEYS_MAX 128

struct binding {
	uint32_t mods;
	xkb_keysym_t sym; /* lower-cased, so Shift+n and N match */
	char cmd[256];
};

struct keymap_cfg {
	struct binding b[KEYS_MAX];
	int n;
	char shell[64]; /* "" = ysh if installed, else /bin/sh */
};

extern const char KEYS_DEFAULT[];

/* Add the bindings in `text` on top of what `out` already holds (later lines
 * override earlier ones for the same combo). Zero `out` first for a fresh map.
 * Returns the number of bad lines that were skipped. */
int keys_parse(const char *text, struct keymap_cfg *out);

const struct binding *keys_find(const struct keymap_cfg *k, uint32_t mods, xkb_keysym_t sym);

#endif
