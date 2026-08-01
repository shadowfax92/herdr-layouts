mod context;
mod herdr;
mod layout;
mod socket;

use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::Deserialize;
use serde_json::json;

use context::FocusedPane;
use herdr::Herdr;
use layout::{equalize_updates, LayoutDescription};
use socket::SocketApi;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Deserialize)]
struct LayoutExportResult {
    layout: LayoutDescription,
}

pub fn split(side: Side, percent: u8) -> Result<()> {
    if !(10..=90).contains(&percent) {
        bail!("split percent must be between 10 and 90");
    }
    let pane = FocusedPane::from_env()?;
    let size = f32::from(percent) / 100.0;
    let first_child_ratio = match side {
        Side::Left => size,
        Side::Right => 1.0 - size,
    };
    let herdr = Herdr::from_env();
    let new_pane_id = herdr.split_right(&pane.pane_id, &pane.cwd, first_child_ratio)?;
    if side == Side::Left {
        herdr.swap(&new_pane_id, &pane.pane_id)?;
    }
    Ok(())
}

pub fn equalize() -> Result<()> {
    let pane = FocusedPane::from_env()?;
    let mut api = SocketApi::from_env()?;
    let result = api.request("layout.export", json!({"pane_id": pane.pane_id}))?;
    let export: LayoutExportResult =
        serde_json::from_value(result).context("invalid layout.export response")?;
    let updates = equalize_updates(&export.layout.root, &pane.pane_id)?;
    for update in updates {
        api.request(
            "layout.set_split_ratio",
            json!({
                "tab_id": export.layout.tab_id,
                "path": update.path,
                "ratio": update.ratio,
            }),
        )?;
    }
    Ok(())
}
