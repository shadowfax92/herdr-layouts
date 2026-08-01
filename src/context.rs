use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PluginContext {
    focused_pane_id: Option<String>,
    focused_pane_cwd: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct FocusedPane {
    pub pane_id: String,
    pub cwd: PathBuf,
}

impl FocusedPane {
    pub fn from_env() -> Result<Self> {
        let plugin_context = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
            .ok()
            .map(|raw| {
                serde_json::from_str::<PluginContext>(&raw)
                    .context("invalid HERDR_PLUGIN_CONTEXT_JSON")
            })
            .transpose()?;
        let pane_id = plugin_context
            .as_ref()
            .and_then(|context| context.focused_pane_id.clone())
            .or_else(|| std::env::var("HERDR_PANE_ID").ok())
            .filter(|value| !value.trim().is_empty())
            .context("plugin action has no focused pane")?;
        let cwd = plugin_context
            .and_then(|context| context.focused_pane_cwd)
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?);

        Ok(Self { pane_id, cwd })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_focused_pane() {
        let context: PluginContext = serde_json::from_str(
            r#"{"focused_pane_id":"w2:p3","focused_pane_cwd":"/tmp/project"}"#,
        )
        .unwrap();

        assert_eq!(context.focused_pane_id.as_deref(), Some("w2:p3"));
        assert_eq!(context.focused_pane_cwd.as_deref(), Some("/tmp/project"));
    }
}
