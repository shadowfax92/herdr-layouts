use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PaneResponse {
    result: PaneResult,
}

#[derive(Debug, Deserialize)]
struct PaneResult {
    pane: Pane,
}

#[derive(Debug, Deserialize)]
struct Pane {
    pane_id: String,
}

pub struct Herdr {
    bin: OsString,
}

impl Herdr {
    pub fn from_env() -> Self {
        Self {
            bin: std::env::var_os("HERDR_BIN_PATH").unwrap_or_else(|| OsString::from("herdr")),
        }
    }

    pub fn split_right(&self, pane_id: &str, cwd: &Path, first_child_ratio: f32) -> Result<String> {
        let args = split_args(pane_id, cwd, first_child_ratio);
        let output = self.run(&args)?;
        let response: PaneResponse =
            serde_json::from_str(&output).context("failed to parse `herdr pane split`")?;
        Ok(response.result.pane.pane_id)
    }

    pub fn swap(&self, source_pane_id: &str, target_pane_id: &str) -> Result<()> {
        self.run(&[
            "pane".into(),
            "swap".into(),
            "--source-pane".into(),
            source_pane_id.into(),
            "--target-pane".into(),
            target_pane_id.into(),
        ])?;
        Ok(())
    }

    fn run(&self, args: &[OsString]) -> Result<String> {
        let output = Command::new(&self.bin)
            .args(args)
            .output()
            .with_context(|| format!("failed to run {}", display_args(args)))?;
        if !output.status.success() {
            bail!(
                "{} failed: {}{}",
                display_args(args),
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8(output.stdout).context("Herdr returned non-UTF-8 output")
    }
}

fn split_args(pane_id: &str, cwd: &Path, first_child_ratio: f32) -> Vec<OsString> {
    vec![
        "pane".into(),
        "split".into(),
        "--pane".into(),
        pane_id.into(),
        "--direction".into(),
        "right".into(),
        "--ratio".into(),
        first_child_ratio.to_string().into(),
        "--cwd".into(),
        cwd.as_os_str().into(),
        "--focus".into(),
    ]
}

fn display_args(args: &[OsString]) -> String {
    std::iter::once(OsStr::new("herdr"))
        .chain(args.iter().map(OsString::as_os_str))
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_focused_right_split() {
        let args = split_args("w2:p3", Path::new("/tmp/project"), 0.7);
        let args = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            args,
            [
                "pane",
                "split",
                "--pane",
                "w2:p3",
                "--direction",
                "right",
                "--ratio",
                "0.7",
                "--cwd",
                "/tmp/project",
                "--focus"
            ]
        );
    }

    #[test]
    fn parses_the_created_pane() {
        let response: PaneResponse =
            serde_json::from_str(r#"{"result":{"type":"pane_info","pane":{"pane_id":"w2:p4"}}}"#)
                .unwrap();

        assert_eq!(response.result.pane.pane_id, "w2:p4");
    }
}
