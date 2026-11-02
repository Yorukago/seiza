# Scripting Seiza: the control socket

Seiza listens on `$XDG_RUNTIME_DIR/seiza.sock`, a Unix stream socket (mode 0600, so only you).
Everything the `seiza` command does goes through it, and so does every satellite (bar, launcher,
carousel...). Anything you can do from the shell you can do from a script, and anything the
satellites know you can know too.

There are two things you can do with it: **send a command**, and **subscribe to the state**.

## Send a command

One line in, one line out, then the connection closes.

```
$ seiza goto 2
ok
$ seiza goto 99
error: no such star
```

From any language, that is: connect, write `seiza <verb> [args]\n`, read one line (`ok` or
`error: <why>`). The `seiza` command is a thin client of exactly this, and exits 0 on `ok`.

```python
import os, socket
s = socket.socket(socket.AF_UNIX)
s.connect(os.environ["XDG_RUNTIME_DIR"] + "/seiza.sock")
s.sendall(b"seiza goto 2\n")
print(s.recv(256).decode())   # "ok\n"
```

**A line that does not start with `seiza` is run as a shell command**, in Seiza's environment, so
the program opens in the current star. That is how the launcher and the keybindings start apps.
(`seiza <verb>` is a command; `firefox` is an app.) A line is at most 1 KiB.

### Commands

Star numbers are 1-based, in the order the stars were made. "Toggle" means the same command
switches it on and off.

| Command | What it does |
|---|---|
| `goto N` | fly the camera to star N |
| `pan DX DY` | move the camera by that many pixels |
| `overview` | toggle the constellation view (all stars, windows hidden) |
| `zoom-reset` | zoom back to 1:1 |
| `focus DX DY` | focus the nearest window in that direction, e.g. `focus -1 0` |
| `focus-window N` | focus window N of the last `switch` list (flies to its star) |
| `cycle-window` | focus the next window |
| `switch` | open the Alt+Tab window list |
| `move-window DX DY` / `resize-window DX DY` | nudge the focused window, in pixels |
| `maximize` / `tile` | toggle maximize for the focused window / tiling for the current star |
| `close-window` | close the focused window |
| `new-star` | add a star |
| `add-star X Y` | add a star at a world position (fails if there is no room) |
| `move-star N X Y` | move star N |
| `delete-star [N]` | delete star N (or the current one); it must have no windows |
| `rename-star N name...` | rename star N (the rest of the line is the name) |
| `link A B` | toggle the thread between stars A and B |
| `figure NAME` | lay the stars out as a built-in or `~/.config/seiza/figures/` figure |
| `new-constellation NAME` / `save NAME` / `load NAME` | constellation files (letters, digits, `_`, `-`) |
| `relaunch` | start the apps remembered for the current star |
| `reload-config` | re-read `seiza.kdl`, keys, palette and the rest (they also reload on save) |
| `help` | open the key help |
| `quit` | quit Seiza |
| `debug-windows`, `debug-pointer X Y [click\|zoom\|down\|up]` | testing aids; the pointer one needs `SEIZA_DEBUG_INPUT=1` |

Unknown commands answer `error: unknown command`; bad arguments answer an error that says what
was expected.

## Subscribe to the state

Send `seiza subscribe\n` and keep the connection open. Seiza sends the current state as **one JSON
object per line**, right away and then again every time it changes (identical repeats are
dropped). You only listen: anything you send on that connection is ignored, so send commands on a
second connection.

```
$ seiza subscribe | jq -c '[.current, .zoom]'
["star 0",1.0000]
["Work",1.0000]
```

```json
{"constellation":"default","current":"Work","overview":false,"zoom":1.0000,
 "cam":[2720.0,180.0],"screen":[1280,720],
 "stars":[{"name":"star 0","x":960,"y":540,"w":1920,"h":1080,"windows":0},
          {"name":"Work","x":3360,"y":540,"w":1920,"h":1080,"windows":2}],
 "links":[[0,1]]}
```

| Field | Meaning |
|---|---|
| `constellation` | name of the open constellation |
| `current` | the star the camera is in (the one under the screen centre; stays as it was in empty space) |
| `overview` | true in the constellation view |
| `zoom` | camera zoom, 1.0 = one world pixel per screen pixel |
| `cam` | camera position, in screen pixels |
| `screen` | the output size in pixels |
| `stars[]` | every star: `name`, centre `x` `y`, size `w` `h` (world pixels), and how many `windows` it holds |
| `links[]` | threads, as pairs of **0-based** indexes into `stars` |

Where something is on screen: `screen position = world position * zoom - cam`.

While the camera glides, the state changes on every step, so a single `goto` can send dozens of
lines. If you only care about the star, compare `current` with the last line you saw.

Things to know:

- A subscriber that does not read fast enough is **dropped** (Seiza never waits for a client). Read
  in a loop, and reconnect if the connection ends.
- If the compositor exits, the stream just ends.
- The state is skipped, not truncated, if it would not fit in 16 KB (hundreds of stars).
- Window titles and app ids are not in the stream. `seiza switch` writes the window list to
  `$XDG_RUNTIME_DIR/seiza-windows` (`star<TAB>app<TAB>title` a line).

## Recipes

Run something when you enter a star:

```sh
last=
seiza subscribe | jq --unbuffered -r .current | while read -r star; do
  [ "$star" != "$last" ] && [ "$star" = "Work" ] && notify-send "back to work"
  last=$star
done
```

Jump to the star with the most windows:

```sh
seiza subscribe | head -1 | jq -r '[.stars | to_entries[] | {i: (.key + 1), w: .value.windows}] | max_by(.w).i' \
  | xargs seiza goto
```

From Rust, `seiza_common::subscribe` and `seiza_common::command` (in `satellites/common`) do this,
reconnecting included; that is what the satellites use.
