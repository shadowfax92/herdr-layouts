//! Exercise the actual CLI boundary, including no-mutation rejection and
//! partial failures. The double tracks pane ownership independently of plans.
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use herdr_layouts::grid::{ApplyOutcome, GridController, GridSize, Snapshot};
use herdr_layouts::herdr::Herdr;
use serde_json::{json, Value};

struct Runtime {
    directory: tempfile::TempDir,
}

impl Runtime {
    fn new(count: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("herdr");
        fs::write(&binary, include_str!("fixtures/herdr-layout.py")).unwrap();
        fs::set_permissions(binary, fs::Permissions::from_mode(0o700)).unwrap();
        let state = json!({"layout": snapshot(count), "log": [], "moves": 0});
        fs::write(directory.path().join("state.json"), state.to_string()).unwrap();
        Self { directory }
    }

    fn controller(&self) -> GridController {
        GridController::new(
            Herdr::new(self.directory.path().join("herdr")),
            "w1:p1".into(),
            self.directory.path().into(),
            self.directory.path().join("grid.lock"),
        )
    }

    fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(self.directory.path().join("state.json")).unwrap())
            .unwrap()
    }

    fn update(&self, key: &str, value: Value) {
        let mut state = self.state();
        state[key] = value;
        fs::write(self.directory.path().join("state.json"), state.to_string()).unwrap();
    }
}

fn snapshot(count: usize) -> Snapshot {
    serde_json::from_value(json!({
        "workspace_id": "w1", "tab_id": "w1:t1", "zoomed": false,
        "focused_pane_id": "w1:p1",
        "area": {"x": 0, "y": 0, "width": 240, "height": 60},
        "panes": (1..=count).map(|n| json!({"pane_id": format!("w1:p{n}"),
            "rect": {"x": (n - 1) * 12, "y": 0, "width": 12, "height": 60}})).collect::<Vec<_>>()
    }))
    .unwrap()
}

#[test]
fn ssh_grid_uses_remote_split_paths_and_a_local_holding_tab() {
    for before in [1, 2] {
        for state in ["live", "stale"] {
            let runtime = Runtime::new(before);
            let cwd = "/__herdr_remote_only__/project";
            runtime.update(
                "source_pane",
                json!({"pane_id":"w1:p1", "cwd":cwd,
                "ssh":{"host":"studio", "link_id":"link-1", "state":state,
                    "local_cwd":runtime.directory.path()}}),
            );
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_herdr-layouts"))
                .args(["grid", "3x1", "--pane", "w1:p1"])
                .env("HERDR_BIN_PATH", runtime.directory.path().join("herdr"))
                .env(
                    "HERDR_SOCKET_PATH",
                    runtime.directory.path().join("test.sock"),
                )
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result = runtime.state();
            let log = result["log"].as_array().unwrap();
            assert!(log
                .iter()
                .filter_map(Value::as_str)
                .filter(|cmd| cmd.starts_with("pane split"))
                .all(|cmd| cmd.contains(&format!("--cwd {cwd} --no-focus"))));
            assert_eq!(result["splits"], 3 - before);
            if before > 1 {
                let holding = log
                    .iter()
                    .filter_map(Value::as_str)
                    .find(|cmd| cmd.starts_with("tab create"))
                    .unwrap();
                assert!(
                    holding.contains(&format!(
                        "--cwd {} --label",
                        runtime.directory.path().display()
                    )),
                    "{holding}"
                );
                assert!(!holding.contains(cwd));
            }
        }
    }
}

#[test]
fn ssh_grid_without_local_cwd_uses_home_only_for_the_holding_tab() {
    for local_cwd in [Some(Value::Null), None] {
        let runtime = Runtime::new(2);
        let mut pane = json!({"pane_id":"w1:p1", "cwd":"/__remote__/project",
            "ssh":{"host":"studio", "state":"live", "link_id":"link"}});
        if let Some(cwd) = local_cwd {
            pane["ssh"]["local_cwd"] = cwd;
        }
        runtime.update("source_pane", pane);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_herdr-layouts"))
            .args(["grid", "3x1", "--pane", "w1:p1"])
            .env("HERDR_BIN_PATH", runtime.directory.path().join("herdr"))
            .env(
                "HERDR_SOCKET_PATH",
                runtime.directory.path().join("test.sock"),
            )
            .env("HOME", runtime.directory.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result = runtime.state();
        let log: Vec<_> = result["log"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(log.iter().any(|cmd| cmd.starts_with("tab create")
            && cmd.contains(&format!(
                "--cwd {} --label",
                runtime.directory.path().display()
            ))));
        assert!(log
            .iter()
            .filter(|cmd| cmd.starts_with("pane split"))
            .all(|cmd| cmd.contains("--cwd /__remote__/project --no-focus")));
    }
}

#[test]
fn local_grid_still_checks_the_source_directory_before_mutating() {
    let runtime = Runtime::new(2);
    runtime.update(
        "source_pane",
        json!({"pane_id":"w1:p1", "cwd":"/__herdr_remote_only__/project"}),
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_herdr-layouts"))
        .args(["grid", "3x1", "--pane", "w1:p1"])
        .env("HERDR_BIN_PATH", runtime.directory.path().join("herdr"))
        .env(
            "HERDR_SOCKET_PATH",
            runtime.directory.path().join("test.sock"),
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Source directory"));
    assert!(runtime.state()["log"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .all(|cmd| cmd.starts_with("pane get") || cmd.starts_with("pane layout")));
}

#[test]
fn mixed_grids_create_new_shells_on_the_captured_source_host() {
    for other_host in [None, Some("other-host")] {
        let runtime = Runtime::new(2);
        runtime.update("source_pane", json!({"pane_id":"w1:p2", "cwd":"/__herdr_remote_only__/project",
            "ssh":{"host":"studio","link_id":"link-1","state":"live", "local_cwd":runtime.directory.path()}}));
        runtime.update("ssh_hosts", json!({"w1:p1":other_host, "w1:p2":"studio"}));
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_herdr-layouts"))
            .args(["grid", "3x2", "--pane", "w1:p2"])
            .env("HERDR_BIN_PATH", runtime.directory.path().join("herdr"))
            .env(
                "HERDR_SOCKET_PATH",
                runtime.directory.path().join("test.sock"),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result = runtime.state();
        for n in 1..=4 {
            assert_eq!(
                result["ssh_hosts"][format!("w1:new{n}")],
                "studio",
                "{result}"
            );
        }
        assert_eq!(
            result["tabs"]["w1:t1"]["panes"].as_array().unwrap().len(),
            6
        );
        assert_eq!(result["ssh_hosts"]["w1:p1"], json!(other_host));
    }
}

#[test]
fn mixed_grids_keep_new_shells_local_when_the_source_is_local() {
    let runtime = Runtime::new(2);
    runtime.update(
        "source_pane",
        json!({"pane_id":"w1:p2", "cwd":runtime.directory.path()}),
    );
    runtime.update("ssh_hosts", json!({"w1:p1":"studio", "w1:p2":null}));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_herdr-layouts"))
        .args(["grid", "2x2", "--pane", "w1:p2"])
        .env("HERDR_BIN_PATH", runtime.directory.path().join("herdr"))
        .env(
            "HERDR_SOCKET_PATH",
            runtime.directory.path().join("test.sock"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = runtime.state();
    assert_eq!(result["splits"], 2);
    for n in 1..=2 {
        assert!(
            result["ssh_hosts"][format!("w1:new{n}")].is_null(),
            "{result}"
        );
    }
    assert_eq!(result["ssh_hosts"]["w1:p1"], "studio");
    assert_eq!(
        result["tabs"]["w1:t1"]["panes"].as_array().unwrap().len(),
        4
    );
    assert!(result["log"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .filter(|cmd| cmd.starts_with("pane split"))
        .all(|cmd| cmd.starts_with("pane split w1:p2 ")));
}

#[test]
fn failed_ssh_shell_staging_retains_original_panes_and_recovery_tab() {
    for (key, value) in [
        ("fail", json!("pane split")),
        ("reject_move", json!(1)),
        ("malformed_move", json!(1)),
    ] {
        let runtime = Runtime::new(2);
        runtime.update("source_pane", json!({"pane_id":"w1:p2", "cwd":"/__herdr_remote_only__/project",
            "ssh":{"host":"studio","link_id":"link-1","state":"live", "local_cwd":runtime.directory.path()}}));
        runtime.update(key, value);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_herdr-layouts"))
            .args(["grid", "3x1", "--pane", "w1:p2"])
            .env("HERDR_BIN_PATH", runtime.directory.path().join("herdr"))
            .env(
                "HERDR_SOCKET_PATH",
                runtime.directory.path().join("test.sock"),
            )
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Holding w1:holding"));
        let result = runtime.state();
        let ids: Vec<_> = result["tabs"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|tab| tab["panes"].as_array().unwrap())
            .map(|pane| pane["pane_id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"w1:p1") && ids.contains(&"w1:p2"));
        assert!(result["log"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .all(|cmd| !cmd.starts_with("pane close")));
    }
}

#[test]
fn exact_grids_preserve_order_and_create_only_missing_shells() {
    for (before, spec) in [
        (1, "5x2"),
        (6, "5x2"),
        (10, "5x2"),
        (3, "1x5"),
        (2, "5x1"),
        (1, "1x1"),
        (13, "7x2"),
    ] {
        let runtime = Runtime::new(before);
        let controller = runtime.controller();
        let size: GridSize = spec.parse().unwrap();
        let expected = size.count().unwrap();
        assert_eq!(
            controller
                .apply(&controller.capture().unwrap(), size)
                .unwrap(),
            ApplyOutcome::Applied {
                created: expected - before
            }
        );
        let state = runtime.state();
        assert_eq!(state["tabs"].as_object().unwrap().len(), 1);
        let after: Snapshot = serde_json::from_value(state["tabs"]["w1:t1"].clone()).unwrap();
        assert_eq!(after.panes.len(), expected);
        assert_eq!(after.focused_pane_id, "w1:p1");
        let mut rows = BTreeMap::new();
        let mut panes = after.panes.clone();
        panes.sort_by_key(|p| (p.rect.y, p.rect.x));
        for (index, pane) in panes.iter().enumerate() {
            if index < before {
                assert_eq!(pane.pane_id, format!("w1:p{}", index + 1));
            }
            rows.entry(pane.rect.y)
                .or_insert_with(Vec::new)
                .push(pane.rect.width);
        }
        assert_eq!(rows.len(), size.rows);
        for widths in rows.values() {
            assert_eq!(widths.len(), size.columns);
            assert!(widths.iter().max().unwrap() - widths.iter().min().unwrap() <= 2);
        }
        assert_eq!(
            state["splits"].as_u64().unwrap_or(0) as usize,
            expected - before
        );
        let log = state["log"].as_array().unwrap();
        assert!(!log
            .iter()
            .any(|c| c.as_str().unwrap().starts_with("tab close")));
    }
}

#[test]
fn ten_into_eight_fails_without_any_mutation_even_when_zoomed() {
    let runtime = Runtime::new(10);
    let mut layout = runtime.state()["layout"].clone();
    layout["zoomed"] = json!(true);
    runtime.update("layout", layout);
    let controller = runtime.controller();
    let before = controller.capture().unwrap();
    let error = controller
        .apply(&before, "4x2".parse().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("More panes already exist"), "{error}");
    assert!(
        error.contains("10 panes") && error.contains("only 8"),
        "{error}"
    );
    assert_eq!(controller.capture().unwrap(), before);
    assert!(runtime.state()["log"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c.as_str().unwrap().starts_with("pane layout")));
}

#[test]
fn stale_preview_and_too_small_grids_do_not_mutate() {
    let runtime = Runtime::new(6);
    let controller = runtime.controller();
    let snapshot = controller.capture().unwrap();
    assert!(controller
        .apply(&snapshot, "100x100".parse().unwrap())
        .is_err());
    let mut tabs = runtime.state()["tabs"].clone();
    tabs["w1:t1"]["area"]["width"] = json!(220);
    runtime.update("tabs", tabs);
    assert!(matches!(
        controller.apply(&snapshot, "5x2".parse().unwrap()).unwrap(),
        ApplyOutcome::Refresh(_)
    ));
    assert_eq!(runtime.state()["moves"], 0);
    assert_eq!(runtime.state()["tabs"].as_object().unwrap().len(), 1);
}

#[test]
fn failed_moves_and_splits_keep_originals_and_report_recovery_tabs() {
    for (key, value) in [
        ("reject_move", json!(1)),
        ("reject_move", json!(5)),
        ("reject_move", json!(9)),
        ("malformed_move", json!(3)),
        ("fail", json!("pane split")),
    ] {
        let runtime = Runtime::new(6);
        runtime.update(key, value);
        let controller = runtime.controller();
        let error = format!(
            "{:#}",
            controller
                .apply(&controller.capture().unwrap(), "5x2".parse().unwrap())
                .unwrap_err()
        );
        assert!(
            error.starts_with("Source w1:t1\nHolding w1:holding"),
            "{error}"
        );
        let state = runtime.state();
        let all = state["tabs"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|tab| tab["panes"].as_array().unwrap())
            .map(|pane| pane["pane_id"].as_str().unwrap())
            .collect::<Vec<_>>();
        for n in 1..=6 {
            assert!(all.contains(&format!("w1:p{n}").as_str()));
        }
        assert!(!state["log"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.as_str().unwrap().contains("close")));
    }
}

#[test]
fn competing_grid_is_rejected_before_mutation() {
    let runtime = Runtime::new(2);
    let file = fs::File::create(runtime.directory.path().join("grid.lock")).unwrap();
    fs4::FileExt::try_lock(&file).unwrap();
    let controller = runtime.controller();
    assert!(controller
        .apply(&controller.capture().unwrap(), "5x2".parse().unwrap())
        .unwrap_err()
        .to_string()
        .contains("Another grid"));
    assert_eq!(runtime.state()["moves"], 0);
}

#[test]
fn parser_rejects_zero_negative_overflow_and_extra_dimensions() {
    for text in [
        "0x2",
        "2x0",
        "-2x3",
        "2x3x4",
        "5",
        "2.5x3",
        "18446744073709551615x2",
    ] {
        assert!(text.parse::<GridSize>().is_err(), "{text}");
    }
    for text in ["5x2", "5X2", " 5 × 2 "] {
        assert_eq!(
            text.parse::<GridSize>().unwrap(),
            GridSize {
                columns: 5,
                rows: 2
            }
        );
    }
}

#[test]
fn dialog_requires_a_valid_grid_and_explicit_enter() {
    use herdr_layouts::grid_ui::{Dialog, Input};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    let mut dialog = Dialog::new(snapshot(10));
    for c in "4x2".chars() {
        assert_eq!(dialog.handle_key(key(KeyCode::Char(c))), Input::Continue);
    }
    assert_eq!(dialog.handle_key(key(KeyCode::Enter)), Input::Continue);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| dialog.render(f)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("More panes already exist"), "{text}");
    dialog.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    for c in "5x2".chars() {
        dialog.handle_key(key(KeyCode::Char(c)));
    }
    assert_eq!(
        dialog.handle_key(key(KeyCode::Enter)),
        Input::Apply("5x2".parse().unwrap())
    );
    assert_eq!(dialog.handle_key(key(KeyCode::Esc)), Input::Close);
}
