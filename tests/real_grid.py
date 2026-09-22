"""Isolated Herdr regression for exact grids and both real popup entrypoints.

Usage: python3 tests/real_grid.py target/release/herdr-layouts [path/to/herdr-menu]
Owns its config, registry, server, PTYs, and test processes. Never contacts the
user session. The optional menu binary exercises Alt-q's foreground handoff.
"""
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

binary = str(Path(sys.argv[1]).resolve())
menu = str(Path(sys.argv[2]).resolve()) if len(sys.argv) > 2 else None
herdr = shutil.which("herdr")
assert herdr, "Herdr must be installed"
session = f"exact-grid-test-{os.getpid()}"


def drain(fd, seconds=0.15):
    deadline = time.monotonic() + seconds
    output = b""
    while time.monotonic() < deadline:
        ready, _, _ = select.select([fd], [], [], max(0, deadline - time.monotonic()))
        if not ready:
            continue
        try:
            chunk = os.read(fd, 262144)
        except OSError:
            break
        if not chunk:
            break
        output += chunk
        if b"\x1b[6n" in chunk:
            os.write(fd, b"\x1b[1;1R")
    return re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", output).decode("utf-8", errors="replace")


def spawn(argv, env, rows, cols):
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(argv[0], argv, env)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    return pid, fd


def reap(pid):
    for _ in range(50):
        result, _ = os.waitpid(pid, os.WNOHANG)
        if result:
            return
        time.sleep(0.05)
    # Only this script's own child, never a discovered/shared Herdr process.
    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)


with tempfile.TemporaryDirectory(prefix="exact-grid-", dir="/tmp") as root:
    env = {key: value for key, value in os.environ.items() if not key.startswith("HERDR_")}
    config = Path(root) / "herdr.toml"
    config.write_text('onboarding = false\n[terminal]\ndefault_shell = "/bin/sh"\n')
    env.update(XDG_CONFIG_HOME=root + "/config", XDG_STATE_HOME=root + "/state",
               HERDR_CONFIG_PATH=str(config), TERM="xterm-256color")

    def cli(*args, text=False):
        result = subprocess.run([herdr, "--session", session, *args], env=env,
                                capture_output=True, text=True, timeout=10)
        assert result.returncode == 0, result.stderr or result.stdout
        return result.stdout if text or not result.stdout.lstrip().startswith("{") else json.loads(result.stdout)

    pid, fd = spawn([herdr, "--session", session], env, 60, 240)
    try:
        drain(fd, 2)
        socket = cli("status", "--json")["server"]["socket"]
        assert Path(socket).is_relative_to(root), "refuse to contact a non-test socket"
        first = cli("pane", "list")["result"]["panes"][0]["pane_id"]
        first_tab = cli("pane", "layout", "--pane", first)["result"]["layout"]["tab_id"]
        cli("pane", "run", first, f"cd {root}; printf 'GRID_KEEP_0\\n'; sleep 300")
        drain(fd, 0.4)
        plugin_env = dict(env, HERDR_BIN_PATH=herdr, HERDR_SOCKET_PATH=socket,
                          HERDR_ENV="1", HERDR_PANE_ID=first)

        def layout():
            return cli("pane", "layout", "--pane", first)["result"]["layout"]

        def ordered(value):
            return [p["pane_id"] for p in sorted(value["panes"], key=lambda p: (p["rect"]["y"], p["rect"]["x"]))]

        def grid(spec, ok=True):
            result = subprocess.run([binary, "grid", spec], env=plugin_env,
                                    capture_output=True, text=True, timeout=30)
            assert (result.returncode == 0) == ok, result.stdout + result.stderr
            return result

        def check(columns, rows, originals, source=first):
            after = layout()
            assert after["tab_id"] == first_tab
            assert len(after["panes"]) == columns * rows, after
            assert ordered(after)[:len(originals)] == originals, after
            assert after["focused_pane_id"] == source, after
            assert not after["zoomed"]
            groups = {}
            for pane in after["panes"]:
                groups.setdefault(pane["rect"]["y"], []).append(pane["rect"])
            assert len(groups) == rows, after
            for group in groups.values():
                assert len(group) == columns, group
                assert max(p["width"] for p in group) - min(p["width"] for p in group) <= 2, group
            assert len(cli("tab", "list")["result"]["tabs"]) == 1
            return after

        original_process = cli("pane", "process-info", "--pane", first)["result"]
        grid("5x2")
        after = check(5, 2, [first])
        assert cli("pane", "process-info", "--pane", first)["result"] == original_process
        panes = ordered(after)
        for index, pane in enumerate(panes[1:], 1):
            info = cli("pane", "get", pane)["result"]["pane"]
            assert Path(info["cwd"]).resolve() == Path(root).resolve(), info
            cli("pane", "run", pane, f"printf 'GRID_KEEP_{index}\\n'; sleep 300")
        drain(fd, 0.4)
        processes = {p: cli("pane", "process-info", "--pane", p)["result"] for p in panes}
        print("PASS 1 → 5x2: nine new shells in the source cwd, original process preserved", flush=True)

        cli("pane", "zoom", "--pane", first, "--on")
        before = layout()
        tabs = cli("tab", "list")
        result = grid("4x2", ok=False)
        assert "More panes already exist" in result.stderr and "10 panes" in result.stderr and "only 8" in result.stderr
        assert layout() == before and cli("tab", "list") == tabs
        grid("100x100", ok=False)
        assert layout() == before
        print("PASS 10 → 8 rejected without changing panes, focus, zoom, or tabs", flush=True)

        grid("5x2")
        check(5, 2, panes)
        grid("6x2")
        check(6, 2, panes)
        for index, pane in enumerate(panes):
            assert cli("pane", "process-info", "--pane", pane)["result"] == processes[pane]
            output = cli("pane", "read", pane, "--source", "recent-unwrapped", "--lines", "500", text=True)
            assert f"GRID_KEEP_{index}" in output
        print("PASS 10 → 10 and 10 → 12: same original IDs, processes, scrollback, focus, and reading order", flush=True)

        # Register just the test binary: no changes to the user's plugin registry.
        plugin = Path(root) / "plugin"
        plugin.mkdir()
        (plugin / "herdr-plugin.toml").write_text(f'''id = "shadowfax.layouts"
name = "Layouts regression"
version = "0.2.0"
description = "Isolated exact-grid regression"
min_herdr_version = "0.9.0"
platforms = ["macos"]
[[actions]]
id = "grid"
title = "Create layout"
contexts = ["pane"]
command = [{json.dumps(binary)}, "open-grid"]
[[panes]]
id = "grid"
title = "Create layout"
placement = "popup"
width = 80
height = 24
command = [{json.dumps(binary)}, "grid-dialog"]
''')
        cli("plugin", "link", str(plugin))
        cli("pane", "zoom", "--pane", panes[3], "--off")
        cli("plugin", "action", "invoke", "shadowfax.layouts.grid")
        native_output = drain(fd, 0.5)
        os.write(fd, b"7x2")
        native_output += drain(fd, 0.3)
        if "Createlayout" not in re.sub(r"\s", "", native_output):
            print(cli("plugin", "log", "--plugin", "shadowfax.layouts", text=True), flush=True)
            raise AssertionError("Native popup failed to open: " + native_output[-1000:])
        os.write(fd, b"\x1bq")
        drain(fd, 0.3)
        check(6, 2, panes, source=panes[3])
        print("PASS native popup preview and cancel leave panes unchanged", flush=True)

        cli("plugin", "action", "invoke", "shadowfax.layouts.grid")
        drain(fd, 0.4)
        os.write(fd, b"7x2\r")
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            drain(fd, 0.2)
            current = layout()
            if len(current["panes"]) == 14 and len(cli("tab", "list")["result"]["tabs"]) == 1:
                break
        else:
            raise AssertionError("Native grid popup did not finish")
        check(7, 2, panes, source=panes[3])
        print("PASS native popup creates 7x2 grid", flush=True)

        if menu:
            menu_root = Path(root) / "menu-plugin"
            menu_root.mkdir()
            (menu_root / "herdr-plugin.toml").write_text(f'''id = "shadowfax.menu"
name = "Menu regression"
version = "0.3.0"
description = "Isolated menu handoff"
min_herdr_version = "0.9.0"
platforms = ["macos"]
[[actions]]
id = "open"
title = "Open menu"
contexts = ["pane"]
command = [{json.dumps(menu)}, "open"]
[[panes]]
id = "menu"
title = "Menu"
placement = "popup"
command = [{json.dumps(menu)}, "menu"]
''')
            cli("plugin", "link", str(menu_root))
            menu_config = Path(cli("plugin", "config-dir", "shadowfax.menu", text=True).strip()) / "config.toml"
            menu_config.parent.mkdir(parents=True, exist_ok=True)
            menu_config.write_text('title = "Test"\nwidth = 80\nheight = 24\n[[items]]\ntype = "redistribute"\nkey = "d"\nlabel = "Redistribute"\n')
            subprocess.run([binary, "install-menu"], env=plugin_env, check=True, capture_output=True, timeout=10)
            cli("pane", "zoom", "--pane", panes[-1], "--off")
            before = layout()
            cli("plugin", "action", "invoke", "shadowfax.menu.open")
            drain(fd, 0.4)
            os.write(fd, b"g")
            output = drain(fd, 0.5)
            os.write(fd, b"4x2\r")
            output += drain(fd, 0.4)
            assert "Morepanesalreadyexist" in re.sub(r"\s", "", output), output
            assert layout() == before
            os.write(fd, b"\x15" + b"5x3\r")  # Ctrl-U replaces the rejected size.
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                output += drain(fd, 0.2)
                if len(layout()["panes"]) == 15 and len(cli("tab", "list")["result"]["tabs"]) == 1:
                    break
            else:
                raise AssertionError("Menu grid did not finish: " + output[-1500:])
            check(5, 3, ordered(before), source=panes[-1])
            for pane in panes:
                assert cli("pane", "process-info", "--pane", pane)["result"] == processes[pane]
            print("PASS menu → g: error stays visible, correction applies, original processes survive", flush=True)
    finally:
        try:
            subprocess.run([herdr, "--session", session, "server", "stop"], env=env,
                           capture_output=True, timeout=10)
        finally:
            os.close(fd)
            reap(pid)
