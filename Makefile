# SEIZA -- the compositor (src/seiza.c) plus small wlroots-free modules that `make test` covers.
#   make        build ./seiza
#   make run    run nested inside your current compositor, with kitty
#   make clipboard   same as run, with the clipboard shared with the host
#   make test   build and run tests/test_*.c
#   make install   seiza + the satellites (starfield wallpaper, bar, session menu, launcher, lock screen, widgets, settings) into $(PREFIX)/bin; Seiza autostarts them

PKGS    := wlroots-0.20 scenefx-0.5 wayland-server xkbcommon
CFLAGS  := -O0 -g -Wall -DWLR_USE_UNSTABLE -Isrc $(shell pkg-config --cflags $(PKGS))
LDLIBS  := $(shell pkg-config --libs $(PKGS))

# Modules the tests link against (everything except the compositor and socket glue).
LIB_SRC := $(filter-out src/seiza.c src/ipc.c,$(wildcard src/*.c))
# Everything the compositor links.
ALL_SRC := $(wildcard src/*.c)

all: seiza

# Protocol header generated from the xdg-shell XML (seiza.c includes it).
src/xdg-shell-protocol.h:
	wayland-scanner server-header /usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml $@

# Vendored (protocols/): wlr-protocols is not installed everywhere. wlroots' layer-shell header includes the result.
src/wlr-layer-shell-unstable-v1-protocol.h: protocols/wlr-layer-shell-unstable-v1.xml
	wayland-scanner server-header $< $@

seiza: $(ALL_SRC) $(wildcard src/*.h) src/xdg-shell-protocol.h src/wlr-layer-shell-unstable-v1-protocol.h
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

tests/bin/%: tests/%.c $(LIB_SRC) $(wildcard src/*.h)
	@mkdir -p tests/bin
	$(CC) -O0 -g -Wall -Isrc $(shell pkg-config --cflags xkbcommon) $< $(LIB_SRC) -o $@ $(shell pkg-config --libs xkbcommon) -lm

test: $(TESTS)
	@for t in $(TESTS); do echo "== $$t"; $$t || exit 1; done; echo "all tests passed"; cargo test --quiet --manifest-path satellites/Cargo.toml

smoke: seiza
	python3 -I tests/smoke_ipc.py

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
	install -Dm755 satellites/target/release/seiza-settings $(PREFIX)/bin/seiza-settings
	install -Dm755 scripts/seiza-wallpaper $(PREFIX)/bin/seiza-wallpaper
	install -Dm644 scripts/wallust/wallust.toml $(PREFIX)/share/seiza/wallust/wallust.toml
	install -Dm644 scripts/wallust/templates/seiza-palette $(PREFIX)/share/seiza/wallust/templates/seiza-palette

clean:
	rm -f seiza src/xdg-shell-protocol.h src/wlr-layer-shell-unstable-v1-protocol.h; rm -rf tests/bin

.PHONY: all run clipboard test smoke install clean
