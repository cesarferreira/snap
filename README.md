<div align="center">
  <h1>snap</h1>

  <p><strong>Window management from the terminal.</strong></p>

  <pre>
$ snap left 50
$ snap right 50
$ snap full
$ snap tile</pre>

  <p><strong>No daemon. No GUI. No config required.</strong></p>

  <p>
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
    <img alt="Rust" src="https://img.shields.io/badge/rust-1.85%2B-orange">
    <img alt="Edition" src="https://img.shields.io/badge/edition-2024-blue">
    <a href="https://crates.io/crates/snap"><img alt="crates.io" src="https://img.shields.io/crates/v/snap.svg"></a>
  </p>

  <p>
    <a href="#install">Install</a>
    &nbsp;·&nbsp;
    <a href="#usage">Usage</a>
    &nbsp;·&nbsp;
    <a href="#configuration">Configuration</a>
    &nbsp;·&nbsp;
    <a href="#keyboard-shortcuts">Keyboard shortcuts</a>
    &nbsp;·&nbsp;
    <a href="#development">Development</a>
  </p>
</div>

---

snap is a fast, minimal command-line window manager for macOS. It gives you
the useful parts of graphical window managers like Rectangle — without a
menu-bar app, a background daemon, or a config file you have to write before
it's usable.

Every command does one thing, then exits:

```
command → manipulate windows → exit
```

It doesn't manage Spaces, replace Mission Control, or keep a persistent
layout tree. It just puts your windows where you want them.

## Install

Requires [Rust](https://rustup.rs) **1.85+** and `~/.cargo/bin` on your `PATH`.

```bash
cargo install snap
```

Verify:

```bash
snap --help
```

The first time snap needs to move a window, macOS will ask you to grant
**Accessibility** permission (System Settings → Privacy & Security →
Accessibility) — for the app that launched it (your terminal), since snap has
no bundle of its own for macOS to attribute the permission to directly.

<details>
<summary><strong>Build from source</strong> — for development or unreleased changes</summary>

```bash
git clone https://github.com/cesarferreira/snap.git
cd snap
cargo install --path . --locked
# or
make install-release
```

Debug install (faster compile, larger binary):

```bash
make install
```

Run without installing:

```bash
make build-release
./target/release/snap
```

</details>

<a id="usage"></a>
## Usage

```
snap <size>
snap <position> [size]
snap <command>
```

### Sizing

Resize the focused window to a percentage of the current display's usable
area, keeping it centered:

```bash
snap 25
snap 50
snap 75
snap 100   # same as `snap full`
```

Omit the percentage to **cycle** 50% → 75% → 25% → 50% → ... each time you
run it, like Rectangle:

```bash
snap
```

Cycling is stateless — snap looks at the window's *current* size to figure
out which step to apply next, so it works correctly no matter what ran it
last, and there's nothing to remember between invocations.

### Positioning

Anchor the focused window to a side, sized to 25/50/75% of the screen:

```bash
snap left 50
snap right 50
snap top 50
snap bottom 50
```

Omit the size to cycle the same way, per side:

```bash
snap left    # 50% → 75% → 25% → 50% → ...
```

### Commands

```bash
snap full     # fill the current display (not native macOS fullscreen)
snap center   # center the window, keep its current size
snap tile     # tile visible windows on the current display
```

`snap tile` lays windows out deterministically: 1 window fills the screen, 2
split 50/50, 3 use a master + stack layout, 4 form a 2×2 grid, and 5+ form a
balanced grid (`columns = ceil(sqrt(n))`). The focused window always gets the
first slot; the rest are ordered top-to-bottom, then left-to-right. Only the
current display is affected — other monitors are left alone.

```bash
snap tile --gap 24   # override the gap between tiles for this run
```

### Output & exit codes

Successful commands print nothing, so snap is safe to bind to hotkeys and use
in scripts. Errors go to stderr.

| Code | Meaning                          |
| ---- | -------------------------------- |
| 0    | Success                          |
| 1    | Runtime failure (e.g. no focused window) |
| 2    | Invalid arguments                |
| 3    | Accessibility permission unavailable |

<a id="configuration"></a>
## Configuration

snap works with zero configuration. If you want to tweak the defaults,
create `~/.config/snap.toml`:

```toml
# Screen-edge and inter-tile padding, in points. Applied to every command
# (left/right/top/bottom/full/center inset from the screen edge; tile uses
# it as both the outer margin and the gap between tiles). Default: 16.
padding = 16

# Width, in points, reserved on the left edge of every display when Stage
# Manager is on, so windows never cover its strip. Ignored entirely when
# Stage Manager is off. Set to 0 to disable the reservation. Default: 150.
stage_manager_width = 150
```

Stage Manager doesn't expose its strip width through any public API, so
`150` is a good starting estimate — adjust it to match what you actually see
on your display.

<a id="keyboard-shortcuts"></a>
## Keyboard shortcuts

snap doesn't register global hotkeys itself — pair it with whatever you
already use:

- [Raycast](https://raycast.com)
- [Karabiner-Elements](https://karabiner-elements.pqrs.org)
- [skhd](https://github.com/koekeishiya/skhd)
- [BetterTouchTool](https://folivora.ai)
- macOS Shortcuts

```
ctrl + alt + left     → snap left 50
ctrl + alt + right    → snap right 50
ctrl + alt + 1        → snap 25
ctrl + alt + 2        → snap 50
ctrl + alt + 3        → snap 75
ctrl + alt + enter    → snap full
ctrl + alt + t        → snap tile
```

<a id="development"></a>
## Development

Common tasks via the `Makefile`:

```bash
make              # check + build + test
make build        # debug build
make build-release
make install      # install debug binary
make install-release
make run ARGS="tile"
make check        # cargo check + clippy
make fmt          # format
make lint         # fmt check + clippy
make test
make clean
make demo         # install + show --help
```

Set `SNAP_DEBUG=1` on any command to print diagnostics to stderr (target
display bounds, windows considered for tiling, requested vs. actual
geometry after each resize) — useful when a window ends up somewhere
unexpected.

Releasing (requires [cargo-release](https://github.com/crate-ci/cargo-release) and [git-cliff](https://github.com/orhun/git-cliff)):

```bash
make release                  # default minor bump
make release LEVEL=patch      # patch bump
make release LEVEL=major      # major bump
```

The pre-release hook regenerates `CHANGELOG.md` with `git-cliff` from your conventional-commit history (grouped into Features, Bug Fixes, etc. per `cliff.toml`) and commits it alongside the version bump. Pushing the resulting `v*` tag triggers the release workflow, which builds the multi-platform binaries and publishes a GitHub Release whose notes are generated by `git-cliff` from the same config.

## License

MIT
