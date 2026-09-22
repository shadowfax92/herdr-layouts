<div align="center">

# 🧱 Herdr Layouts

**Exact pane grids and tmux-style layout shortcuts for Herdr.**

[![Herdr 0.9.0+](https://img.shields.io/badge/Herdr-0.9.0%2B-6c71c4)](https://herdr.dev)
[![Rust](https://img.shields.io/badge/built%20with-Rust-b7410e)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

</div>

Herdr Layouts creates exact grids and provides three useful tmux pane commands:

| Key | Result |
| --- | --- |
| `Alt-q`, then `g` | Type a grid such as `5x2`, preview it, and press Enter |
| `prefix V` | Create and focus a 30%-wide pane on the left |
| `prefix B` | Create and focus a 30%-wide pane on the right |
| `prefix =` | Spread the focused pane and its adjacent peers evenly |

Equalization follows tmux's `select-layout -E` behavior. It equalizes the contiguous row or column nearest the focused pane while preserving perpendicular splits.

## Install

Requires macOS or Linux (including WSL), Herdr 0.9.0 or newer, and a Rust toolchain.

On Windows, run Herdr and this plugin inside WSL. Native Windows is not supported.

```sh
herdr plugin install shadowfax92/herdr-layouts
```

If Herdr Menu is installed, add **Create layout…** to the existing Alt-q menu:

```sh
herdr plugin action invoke shadowfax.layouts.install-menu
```

This backs up the menu config and appends `g`, preserving existing items and
comments. It refuses a conflicting `g` binding. Running it again is a no-op.
No global shortcut changes or server restart are needed.

Without Herdr Menu, open the standalone popup:

```sh
herdr plugin action invoke shadowfax.layouts.grid
```

## Create an exact grid

`5x2` means **5 columns × 2 rows = 10 panes**. Type the size, inspect the preview,
then press Enter. Escape or Alt-q cancels; Ctrl-u clears the input.

- With 6 panes, `5x2` keeps all 6 and adds 4 empty shells.
- With 10 panes, `5x2` rearranges those 10 without adding shells.
- With 10 panes, `4x2` errors: **More panes already exist: this tab has 10 panes,
  but 4x2 holds only 8.** No pane, focus, or zoom changes occur.

The preview labels new shells and highlights the source pane. All tiled panes
participate, in visual reading order. New shells inherit the source pane's
current working directory; existing processes, pane IDs, and scrollback survive.
The source pane regains focus after applying. Applying unzooms the tab.

The direct CLI uses the same executor:

```sh
./target/release/herdr-layouts grid 5x2
./target/release/herdr-layouts grid 4x3 --pane w1:p2
```

The notation matches `layouts grid`, but this plugin does not require the
separate `layouts` CLI. It calls Herdr directly and creates only missing shells.
It briefly stages existing panes in a `layouts-grid-…` tab because Herdr cannot
move panes within one tab. Cleanup verifies membership and closes only the
temporary shell. It never replaces a live terminal or closes a whole tab.

A changed preview requires another Enter. Grids below 12 columns × 5 lines per
pane are rejected before mutation. If a multi-step operation fails, the error
shows the source and holding tab IDs; originals remain running. Inspect those
tabs and move panes back manually before retrying. There is no automatic rollback.

## Split and equalize shortcuts

Add the actions to `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+shift+v"
type = "plugin_action"
command = "shadowfax.layouts.split-left-30"
description = "Split 30% pane on the left"

[[keys.command]]
key = "prefix+shift+b"
type = "plugin_action"
command = "shadowfax.layouts.split-right-30"
description = "Split 30% pane on the right"

[[keys.command]]
key = "prefix+="
type = "plugin_action"
command = "shadowfax.layouts.equalize"
description = "Spread nearby panes evenly"
```

Reload the running server:

```sh
herdr server reload-config
```

New panes inherit the focused pane's working directory. The split command also accepts a custom percentage when run directly:

```sh
HERDR_PANE_ID=w1:p1 herdr-layouts split right --percent 40
```

## Local development

```sh
git clone https://github.com/shadowfax92/herdr-layouts.git
cd herdr-layouts
herdr plugin link .
herdr plugin action invoke shadowfax.layouts.install-menu
```

Run the local gate:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

The optional real-server regression uses its own named server, config, registry,
and test processes. It never contacts your working session:

```sh
python3 tests/real_grid.py target/release/herdr-layouts ../herdr-menu/target/release/herdr-menu
```

Omit the final menu binary argument to test only the CLI and standalone popup.
The regular tests require Python 3 for the stateful Herdr CLI double.

## License

[MIT](LICENSE)
