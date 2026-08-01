<div align="center">

# 🧱 Herdr Layouts

**Tmux-style pane layout shortcuts for Herdr.**

[![Herdr 0.7.5+](https://img.shields.io/badge/Herdr-0.7.5%2B-6c71c4)](https://herdr.dev)
[![Rust](https://img.shields.io/badge/built%20with-Rust-b7410e)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

</div>

Herdr Layouts recreates three useful tmux pane commands:

| Key | Result |
| --- | --- |
| `prefix V` | Create and focus a 30%-wide pane on the left |
| `prefix B` | Create and focus a 30%-wide pane on the right |
| `prefix =` | Spread the focused pane and its adjacent peers evenly |

Equalization follows tmux's `select-layout -E` behavior. It equalizes the contiguous row or column nearest the focused pane while preserving perpendicular splits.

## Install

Requires macOS, Herdr 0.7.5 or newer, and a Rust toolchain.

```sh
herdr plugin install shadowfax92/herdr-layouts
```

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
```

Run the local gate:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

## License

[MIT](LICENSE)
