/* SEIZA: key bindings file parser. See keys.h. */
#include <ctype.h>
#include <stdio.h>
#include <string.h>
#include <strings.h>
#include "keys.h"

/* The built-in bindings, in the user's file syntax. Super+letter-O is the
 * overview, Super+0 (zero) resets zoom: yoru has mixed those up before. */
const char KEYS_DEFAULT[] =
	"Super+Escape = seiza quit\n"
	"Super+Left    = seiza pan -100 0\n"
	"Super+Right   = seiza pan 100 0\n"
	"Super+Up      = seiza pan 0 -100\n"
	"Super+Down    = seiza pan 0 100\n"
	"Super+0       = seiza zoom-reset\n"
	"Super+o       = seiza overview\n"
	"Super+n       = seiza new-star\n"
	"Super+x       = seiza delete-star\n"
	"Super+r       = seiza relaunch\n"
	"Super+Shift+Left  = seiza move-window -40 0\n"
	"Super+Shift+Right = seiza move-window 40 0\n"
	"Super+Shift+Up    = seiza move-window 0 -40\n"
	"Super+Shift+Down  = seiza move-window 0 40\n"
	"Super+Ctrl+Left   = seiza resize-window -40 0\n"
	"Super+Ctrl+Right  = seiza resize-window 40 0\n"
	"Super+Ctrl+Up     = seiza resize-window 0 -40\n"
	"Super+Ctrl+Down   = seiza resize-window 0 40\n"
	"Super+Ctrl+h  = seiza focus -1 0\n"
	"Super+Ctrl+l  = seiza focus 1 0\n"
	"Super+Ctrl+k  = seiza focus 0 -1\n"
	"Super+Ctrl+j  = seiza focus 0 1\n"
	"Super+h       = seiza-help\n"
	"Super+l       = seiza-launcher\n"
	"Super+v       = seiza-launcher --clip\n"
	"Super+Shift+v = seiza-launcher --pass\n"
	"Super+Tab     = seiza switch\n"
	"Super+Shift+l = seiza-lock\n"
	"Super+s       = seiza-settings\n"
	"Super+d       = seiza-panel\n"
	"Super+c       = seiza-constellations\n"
	"Super+g       = seiza-hh\n"
	"Super+k       = seiza-carousel\n"
	"Super+1       = seiza goto 1\n"
	"Super+2       = seiza goto 2\n"
	"Super+3       = seiza goto 3\n"
	"Super+4       = seiza goto 4\n"
	"Super+5       = seiza goto 5\n"
	"Super+6       = seiza goto 6\n"
	"Super+7       = seiza goto 7\n"
	"Super+8       = seiza goto 8\n"
	"Super+9       = seiza goto 9\n"
	"Super+p       = seiza-shot area\n"
	"Super+Shift+p = seiza-shot screen\n"
	"Super+Ctrl+p  = seiza-shot record\n"
	"Super+q       = seiza close-window\n"
	"Super+m       = seiza maximize\n"
	"Super+t       = seiza tile\n"
	"Super+F1      = seiza cycle-window\n"
	"Super+Return  = kitty\n"
	"XF86AudioRaiseVolume = wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+\n"
	"XF86AudioLowerVolume = wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-\n"
	"XF86AudioMute        = wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle\n"
	"XF86MonBrightnessUp   = brightnessctl set 5%+\n"
	"XF86MonBrightnessDown = brightnessctl set 5%-\n"
	"XF86AudioPlay  = playerctl play-pause\n"
	"XF86AudioNext  = playerctl next\n"
	"XF86AudioPrev  = playerctl previous\n";

static char *trim(char *s) {
	while (isspace((unsigned char)*s)) {
		s++;
	}
	size_t n = strlen(s);
	while (n > 0 && isspace((unsigned char)s[n - 1])) {
		s[--n] = '\0';
	}
	return s;
}

/* One modifier word, or 0 if it is not one. */
static uint32_t mod_from_name(const char *w) {
	if (!strcasecmp(w, "alt")) return KMOD_ALT;
	if (!strcasecmp(w, "shift")) return KMOD_SHIFT;
	if (!strcasecmp(w, "ctrl") || !strcasecmp(w, "control")) return KMOD_CTRL;
	if (!strcasecmp(w, "super") || !strcasecmp(w, "logo") || !strcasecmp(w, "mod4")) return KMOD_SUPER;
	return 0;
}

/* "Alt+Shift+Left" -> mods + sym. 0 ok, -1 bad. */
static int parse_combo(char *combo, uint32_t *mods, xkb_keysym_t *sym) {
	*mods = 0;
	char *key = combo, *plus;
	while ((plus = strchr(key, '+')) != NULL && plus != key) {
		*plus = '\0';
		uint32_t m = mod_from_name(trim(key));
		if (!m) {
			return -1;
		}
		*mods |= m;
		key = plus + 1;
	}
	key = trim(key);
	/* A literal '+' key must be written "plus" (its xkb name). */
	xkb_keysym_t s = xkb_keysym_from_name(key, XKB_KEYSYM_CASE_INSENSITIVE);
	if (s == XKB_KEY_NoSymbol) {
		return -1;
	}
	*sym = xkb_keysym_to_lower(s);
	return 0;
}

const struct binding *keys_find(const struct keymap_cfg *k, uint32_t mods, xkb_keysym_t sym) {
	sym = xkb_keysym_to_lower(sym);
	for (int i = 0; i < k->n; i++) {
		if (k->b[i].mods == mods && k->b[i].sym == sym) {
			return &k->b[i];
		}
	}
	return NULL;
}

int keys_parse(const char *text, struct keymap_cfg *out) {
	int bad = 0;
	const char *p = text;
	while (*p) {
		char line[1024];
		size_t len = strcspn(p, "\n");
		snprintf(line, sizeof(line), "%.*s", (int)(len < sizeof(line) ? len : sizeof(line) - 1), p);
		int too_long = len >= sizeof(line);
		p += len + (p[len] == '\n');

		char *l = trim(line);
		if (*l == '\0' || *l == '#') {
			continue;
		}
		char *eq = strchr(l, '=');
		if (too_long || !eq) {
			bad++;
			continue;
		}
		*eq = '\0';
		char *left = trim(l), *value = trim(eq + 1);
		if (*left == '\0' || *value == '\0') {
			bad++;
			continue;
		}
		if (strcasecmp(left, "shell") == 0) {
			if (strlen(value) >= sizeof(out->shell)) {
				bad++;
			} else {
				strcpy(out->shell, value);
			}
			continue;
		}
		uint32_t mods;
		xkb_keysym_t sym;
		if (strlen(value) >= sizeof(out->b[0].cmd) || parse_combo(left, &mods, &sym) != 0) {
			bad++;
			continue;
		}
		struct binding *b = (struct binding *)keys_find(out, mods, sym);
		if (!b) {
			if (out->n >= KEYS_MAX) {
				bad++;
				continue;
			}
			b = &out->b[out->n++];
			b->mods = mods;
			b->sym = sym;
		}
		strcpy(b->cmd, value);
	}
	return bad;
}
