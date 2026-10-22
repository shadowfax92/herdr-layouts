#!/usr/bin/env python3
"""Stateful CLI boundary double: moves preserve leaf IDs; only owned roots close.

The fixture models public response envelopes and split geometry, not plugin
internals. Each Rust test gets its own directory/state so tests can run in parallel.
"""
import json
import pathlib
import sys

path = pathlib.Path(__file__).with_name("state.json")
state = json.loads(path.read_text())
args = sys.argv[1:]
command = " ".join(args)
state["log"].append(command)


def option(name, default=None):
    return args[args.index(name) + 1] if name in args else default


def split_tree(ids):
    if len(ids) == 1:
        return ids[0]
    mid = (len(ids) + 1) // 2
    return ["right", mid / len(ids), split_tree(ids[:mid]), split_tree(ids[mid:])]


def leaves(tree):
    return [tree] if isinstance(tree, str) else leaves(tree[2]) + leaves(tree[3])


def remove(tree, pane):
    if isinstance(tree, str):
        return None if tree == pane else tree
    first, second = remove(tree[2], pane), remove(tree[3], pane)
    if first is None:
        return second
    if second is None:
        return first
    return [tree[0], tree[1], first, second]


def insert(tree, target, pane, direction, ratio):
    if isinstance(tree, str):
        return [direction, ratio, tree, pane] if tree == target else tree
    return [tree[0], tree[1], insert(tree[2], target, pane, direction, ratio),
            insert(tree[3], target, pane, direction, ratio)]


def rectangles(tree, rect):
    if isinstance(tree, str):
        return [{"pane_id": tree, "rect": rect}]
    first, second = dict(rect), dict(rect)
    dimension, position = ("width", "x") if tree[0] == "right" else ("height", "y")
    first[dimension] = round(rect[dimension] * tree[1])
    second[dimension] -= first[dimension]
    second[position] += first[dimension]
    return rectangles(tree[2], first) + rectangles(tree[3], second)


if "tabs" not in state:
    layout = state["layout"]
    state["tabs"] = {layout["tab_id"]: layout}
    ids = [p["pane_id"] for p in sorted(layout["panes"], key=lambda p: (p["rect"]["y"], p["rect"]["x"]))]
    state["trees"] = {layout["tab_id"]: split_tree(ids)}


def locate(pane):
    return next(tab for tab, tree in state["trees"].items() if pane in leaves(tree))


def refresh():
    for tab, tree in state["trees"].items():
        layout = state["tabs"][tab]
        layout["panes"] = rectangles(tree, layout["area"])
        if layout["focused_pane_id"] not in leaves(tree):
            layout["focused_pane_id"] = leaves(tree)[0]


def reply(value, code=0):
    path.write_text(json.dumps(state))
    if code:
        print(value, file=sys.stderr)
    else:
        print(json.dumps({"result": value}))
    sys.exit(code)


if state.get("fail") and command.startswith(state["fail"]):
    reply("simulated CLI failure", 9)

if args[:2] == ["pane", "get"]:
    reply({"pane": state["source_pane"]})
elif args[:2] == ["pane", "layout"]:
    pane = option("--pane")
    reply({"layout": state["tabs"][locate(pane)]})
elif args[:2] == ["tab", "get"]:
    reply({"tab": {"tab_id": args[2], "label": "test-tab"}})
elif args[:2] == ["tab", "create"]:
    layout = dict(state["tabs"]["w1:t1"])
    layout.update(tab_id="w1:holding", focused_pane_id="w1:temporary", zoomed=False)
    state["tabs"]["w1:holding"] = layout
    state["trees"]["w1:holding"] = "w1:temporary"
    refresh()
    reply({"tab": {"tab_id": "w1:holding"}, "root_pane": {"pane_id": "w1:temporary"}})
elif args[:2] == ["pane", "move"]:
    state["moves"] += 1
    if state.get("reject_move") == state["moves"]:
        reply({"move_result": {"changed": False, "reason": "not_found"}})
    pane, target = args[2], option("--target-pane")
    source, destination = locate(pane), option("--tab")
    if source == destination or state["tabs"][source]["zoomed"] or state["tabs"][destination]["zoomed"]:
        reply({"move_result": {"changed": False, "reason": "same_or_zoomed"}})
    assert locate(target) == destination
    ratio = float(option("--ratio"))
    assert 0.1 <= ratio <= 0.9
    state["trees"][source] = remove(state["trees"][source], pane)
    state["trees"][destination] = insert(state["trees"][destination], target, pane, option("--split"), ratio)
    refresh()
    if state.get("malformed_move") == state["moves"]:
        reply({"move_result": {"changed": True, "pane": {"pane_id": "unexpected"}}})
    reply({"move_result": {"changed": True, "pane": {"pane_id": pane}}})
elif args[:2] == ["pane", "split"]:
    state["splits"] = state.get("splits", 0) + 1
    pane = f'w1:new{state["splits"]}'
    target = args[2]
    if "ssh_hosts" in state:
        state["ssh_hosts"][pane] = state["ssh_hosts"].get(target)
    tab = locate(target)
    state["trees"][tab] = insert(state["trees"][tab], target, pane,
                                 option("--direction"), float(option("--ratio")))
    refresh()
    reply({"pane": {"pane_id": pane}})
elif args[:2] == ["pane", "zoom"]:
    pane = option("--pane")
    state["tabs"][locate(pane)]["zoomed"] = "--on" in args
    state["tabs"][locate(pane)]["focused_pane_id"] = pane
    reply({"type": "pane_zoom"})
elif args[:2] == ["pane", "close"]:
    pane = args[2]
    assert pane == "w1:temporary", "an original pane must never be closed"
    tab = locate(pane)
    tree = remove(state["trees"][tab], pane)
    if tree is None:
        del state["trees"][tab]
        del state["tabs"][tab]
    else:
        state["trees"][tab] = tree
        refresh()
    reply({"type": "pane_closed"})
elif args[:2] == ["agent", "focus"]:
    reply("agent_not_found: these fixtures are ordinary shell panes", 1)
elif args[:2] == ["tab", "focus"]:
    state["focused_tab"] = args[2]
    reply({"type": "tab_focused"})
else:
    reply("unsupported fake command: " + command, 2)
