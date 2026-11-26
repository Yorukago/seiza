# Seiza

A Wayland compositor written in C on top of wlroots and scenefx, plus a set of
Rust "satellites" that make up the rest of the desktop: bar, launcher, lock
screen, widgets, settings and more!

<!-- *insert a screenshot or short gif of the desktop...haha* -->

## Features

- Compositor (`src/seiza.c`): Basically made on wlroots, subject to a LOT of refactoring, not even the devil wants to touch this
- Satellites: bar, launcher, lock screen, OSD, notifications, widgets, settings, help, diary, music player (wonder), password vault (pass)
- Constellations: Windows live inside the stars, stars live inside contellations, you can make your own constellations and use them too!
- Hot-reloaded configuration: the config dir is being watched and applied like...200ms after being saved

## Requirements

- wlroots 0.20, scenefx 0.5, libinput, wayland-server, xkbcommon, xcb (+ icccm, ewmh)
- A C compiler, `make`, `pkg-config`
- Rust toolchain (`cargo`) for the satellites
- Optional: `wl-clipboard` (clipboard sharing in nested mode), 

## Build and try it

```sh
make                 # build ./seiza
make run             # run nested inside your current compositor, with kitty
make clipboard       # same, with the host clipboard shared
make test            # satellite unit tests (cargo test)
```

Override the keyboard layout for the nested session with `make run LAYOUT=us`.

## Install

```sh
make install           # seiza + satellites into ~/.local/bin (PREFIX to change)
make install-session   # list Seiza in SDDM/GDM (needs sudo)
```

Seiza autostarts its satellites. From inside a session, `seiza-update`
rebuilds, installs and restarts them.

## Usage

<!-- TODO: the 5-10 keybindings people need first (Super+... ), or point to seiza-help -->
(for sake of ease of use, ill call the SUPER key the meta key)

- meta + H           help
- meta + L           launcher (+ tab inside is star listed)
- meta + V           clipboard
- meta + G           HueHaven (refer to my other github repo)

- meta + S           settings
- meta + enter       kitty

- windows cascade if you open lots on top of eachother - like windows
- you can resize windows using meta + right click
- you can move windows using meta + left click

- to move the camera, unfocus from the current window (if any), left click and drag
- to zoom in/out, unfocus from the current window (if any), scroll up/down

More keybinds on seiza-help!!

## Project layout

| Path | What lives there |
|------|------------------|
| `src/` | the compositor (C) |
| `satellites/` | Rust workspace: one crate per satellite, plus `common` |
| `figures/` | built-in figures (`.fig`) compiled into the compositor |
| `protocols/` | vendored Wayland protocol XML |
| `portals/` | xdg-desktop-portal configuration |
| `scripts/` | helpers, desktop entries, `seiza-update` |
| `docs/ipc.md` | the IPC between the compositor and satellites |

## Status

it works, its daily drivable, some things like screensharing on vesktop/discord might not work (as of 26th nov 2026)

its missing a polkit agent (written, but untested and not merged yet) and a better screenshare picker...

i was planning to make it have support for multiple monitors, PLEASE DONT USE ANOTHER MONITOR WHILE TRYING SEIZA!!! 

i was also planning to have a "hyprland" mode, where windows instead of cascading, they'd behave like hyprland - tiled and gliding~

seiza-pass is still read only, practically a stub

im also planning to refactor seiza.c since not even i wanna touch it at this point

maybe a XDG autostart - where apps would autostart based on the star on which constellation 


## Warnings

again as i said, seiza isnt tested on multiple monitors, so it could misbehave or act nicely (highly doubt.....)

sometimes when suspending my laptop sometimes seiza just stops wanting to exist (honestly same), some AMD laptops need panel self-refresh off
to resume

still this is pretty much a love project, made to integrate both ysh (sigh...another repo to put) and to show love for the game of making "things" 
that might make my life easier (or harder....im looking at u screenshare.)


## Credits

i gotta thank my friends that made me finally make this repo, usually im too shy/embarrassed to show off my work

i gotta also thank Claude, from anthropic, that helped quite a lot in the making of this, legit id take a thousand years to make this if it wasnt for 
my bestie Claudie!!! 

anyone that indirectly and directly made and contributed to Rust, wlroots, Maple-Mono Font, and everyone else!!
and to u dear reader


## License

MIT, see [LICENSE](LICENSE).
