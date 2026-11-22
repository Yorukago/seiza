# SEIZA -- the compositor (src/seiza.c) plus small wlroots-free modules that `make test` covers.
#   make        build ./seiza
#   make run    run nested inside your current compositor, with kitty
#   make clipboard   same as run, with the clipboard shared with the host
#   make test   build and run tests/test_*.c
#   make install   seiza + the satellites (starfield wallpaper, bar, session menu, launcher, lock screen, widgets, settings, constellation editor, constellation view) into $(PREFIX)/bin; Seiza autostarts them

PKGS    := wlroots-0.20 scenefx-0.5 wayland-server xkbcommon xcb xcb-icccm xcb-ewmh
CFLAGS  := -O2 -g -Wall -DWLR_USE_UNSTABLE -Isrc $(shell pkg-config --cflags $(PKGS))
LDLIBS  := $(shell pkg-config --libs $(PKGS))

# Modules the tests link against (everything except the compositor and socket glue).
LIB_SRC := $(filter-out src/seiza.c src/ipc.c,$(wildcard src/*.c))
# Everything the compositor links.
ALL_SRC := $(wildcard src/*.c)

all: seiza

# The built-in figures, from figures/*.fig (figure.c includes the table).
src/figures-builtin.h: $(wildcard figures/*.fig) scripts/gen-figures.sh
	sh scripts/gen-figures.sh figures > $@

# Protocol header generated from the xdg-shell XML (seiza.c includes it).
src/xdg-shell-protocol.h:
	wayland-scanner server-header /usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml $@

# Vendored (protocols/): wlr-protocols is not installed everywhere. wlroots' layer-shell header includes the result.
src/wlr-layer-shell-unstable-v1-protocol.h: protocols/wlr-layer-shell-unstable-v1.xml
	wayland-scanner server-header $< $@

seiza: $(ALL_SRC) $(wildcard src/*.h) src/figures-builtin.h src/xdg-shell-protocol.h src/wlr-layer-shell-unstable-v1-protocol.h
	$(CC) $(CFLAGS) $(ALL_SRC) -o $@ $(LDLIBS)

# The nested compositor doesn't inherit Hyprland's keyboard layout, so pass it
# in (xkbcommon reads XKB_DEFAULT_*). Override with: make run LAYOUT=us
LAYOUT ?= pt

run: seiza
	XKB_DEFAULT_LAYOUT=$(LAYOUT) ./seiza -s kitty

# Like `make run`, plus two-way text clipboard sync with the host (scripts/clipbridge, needs wl-clipboard).
clipboard: seiza
	XKB_DEFAULT_LAYOUT=$(LAYOUT) HOST_WAYLAND_DISPLAY=$$WAYLAND_DISPLAY ./seiza -s 'scripts/clipbridge & exec kitty'

TESTS :=$(patsubst tests/%.c,tests/bin/%,$(wildcard tests/test_*.c))

tests/bin/%: tests/%.c $(LIB_SRC) $(wildcard src/*.h) src/figures-builtin.h
	@mkdir -p tests/bin
	$(CC) -O0 -g -Wall -Isrc $(shell pkg-config --cflags xkbcommon) $< $(LIB_SRC) -o $@ $(shell pkg-config --libs xkbcommon) -lm

test: $(TESTS)
	@for t in $(TESTS); do echo "== $$t"; $$t || exit 1; done; echo "all tests passed"; cargo test --quiet --manifest-path satellites/Cargo.toml

smoke: seiza
	python3 -I tests/smoke_ipc.py
	python3 -I tests/smoke_monitors.py

PREFIX ?= $(HOME)/.local

install: seiza
	cargo build --release --manifest-path satellites/Cargo.toml
	install -Dm755 seiza $(PREFIX)/bin/seiza
	install -Dm755 satellites/target/release/seiza-starfield $(PREFIX)/bin/seiza-starfield
	install -Dm755 satellites/target/release/seiza-bar $(PREFIX)/bin/seiza-bar
	install -Dm755 satellites/target/release/seiza-session $(PREFIX)/bin/seiza-session
	install -Dm755 satellites/target/release/seiza-launcher $(PREFIX)/bin/seiza-launcher
	install -Dm755 satellites/target/release/seiza-lock $(PREFIX)/bin/seiza-lock
	install -Dm755 satellites/target/release/seiza-widgets $(PREFIX)/bin/seiza-widgets
	install -Dm755 satellites/target/release/seiza-osd $(PREFIX)/bin/seiza-osd
	install -Dm755 satellites/target/release/seiza-notify $(PREFIX)/bin/seiza-notify
	install -Dm755 satellites/target/release/seiza-panel $(PREFIX)/bin/seiza-panel
	install -Dm755 satellites/target/release/seiza-diary $(PREFIX)/bin/seiza-diary
	install -Dm755 satellites/target/release/seiza-settings $(PREFIX)/bin/seiza-settings
	install -Dm755 satellites/target/release/seiza-constellations $(PREFIX)/bin/seiza-constellations
	install -Dm755 satellites/target/release/seiza-carousel $(PREFIX)/bin/seiza-carousel
	install -Dm755 satellites/target/release/seiza-conf $(PREFIX)/bin/seiza-conf
	install -Dm755 satellites/target/release/seiza-help $(PREFIX)/bin/seiza-help
	install -Dm755 satellites/target/release/seiza-hh $(PREFIX)/bin/seiza-hh
	install -Dm755 satellites/target/release/seiza-sleep $(PREFIX)/bin/seiza-sleep
	install -Dm644 portals/seiza-portals.conf $(HOME)/.config/xdg-desktop-portal/seiza-portals.conf
	test -e $(HOME)/.config/xdg-desktop-portal-wlr/config || install -Dm644 portals/xdpw.conf $(HOME)/.config/xdg-desktop-portal-wlr/config
	install -Dm755 scripts/seiza-shot $(PREFIX)/bin/seiza-shot
	install -Dm755 scripts/seiza-wallpaper $(PREFIX)/bin/seiza-wallpaper
	install -Dm755 scripts/seiza-clip-store $(PREFIX)/bin/seiza-clip-store
	install -Dm644 scripts/wallust/wallust.toml $(PREFIX)/share/seiza/wallust/wallust.toml
	install -Dm644 scripts/wallust/templates/seiza-palette $(PREFIX)/share/seiza/wallust/templates/seiza-palette

clean:
	rm -f seiza src/figures-builtin.h src/xdg-shell-protocol.h src/wlr-layer-shell-unstable-v1-protocol.h; rm -rf tests/bin

.PHONY: all run clipboard test smoke install clean
