# SEIZA -- the compositor (src/seiza.c) plus small wlroots-free modules that `make test` covers.
#   make        build ./seiza
#   make run    run nested inside your current compositor, with kitty
#   make test   build and run tests/test_*.c

PKGS    := wlroots-0.20 wayland-server xkbcommon
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

seiza: $(ALL_SRC) $(wildcard src/*.h) src/xdg-shell-protocol.h
	$(CC) $(CFLAGS) $(ALL_SRC) -o $@ $(LDLIBS)

# The nested compositor doesn't inherit Hyprland's keyboard layout, so pass it
# in (xkbcommon reads XKB_DEFAULT_*). Override with: make run LAYOUT=us
LAYOUT ?= pt

run: seiza
	XKB_DEFAULT_LAYOUT=$(LAYOUT) ./seiza -s kitty

TESTS := $(patsubst tests/%.c,tests/bin/%,$(wildcard tests/test_*.c))

tests/bin/%: tests/%.c $(LIB_SRC) $(wildcard src/*.h)
	@mkdir -p tests/bin
	$(CC) -O0 -g -Wall -Isrc $(shell pkg-config --cflags xkbcommon) $< $(LIB_SRC) -o $@ $(shell pkg-config --libs xkbcommon)

test: $(TESTS)
	@for t in $(TESTS); do echo "== $$t"; $$t || exit 1; done; echo "all tests passed"

smoke: seiza
	python3 -I tests/smoke_ipc.py

clean:
	rm -f seiza src/xdg-shell-protocol.h; rm -rf tests/bin

.PHONY: all run test smoke clean
