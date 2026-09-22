//! Exact grids preserve live terminals and fill only unoccupied cells with shells.
//! Herdr cannot move panes within one tab, so originals are briefly staged in a
//! holding tab. Only our holding shell is ever closed; failures retain live panes.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::herdr::Herdr;

/// Column-first notation shared by the direct CLI and interactive preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub columns: usize,
    pub rows: usize,
}

impl FromStr for GridSize {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let normalized = value.trim().to_lowercase().replace('×', "x");
        let (columns, rows) = normalized
            .split_once('x')
            .context("Enter columns x rows, for example 5x2")?;
        let size = Self {
            columns: columns
                .trim()
                .parse()
                .context("Columns must be a positive whole number")?,
            rows: rows
                .trim()
                .parse()
                .context("Rows must be a positive whole number")?,
        };
        size.count()?;
        Ok(size)
    }
}

impl GridSize {
    pub fn count(self) -> Result<usize> {
        if self.columns == 0 || self.rows == 0 {
            bail!("Columns and rows must be at least 1");
        }
        self.columns
            .checked_mul(self.rows)
            .context("Grid is too large")
    }

    pub fn validate(self, snapshot: &Snapshot) -> Result<usize> {
        let count = self.count()?;
        if snapshot.panes.len() > count {
            bail!(
                "More panes already exist: this tab has {} panes, but {}x{} holds only {count}.",
                snapshot.panes.len(),
                self.columns,
                self.rows
            );
        }
        // Frames consume terminal cells too. Refuse unusable grids before
        // creating a holding tab, unzooming, or moving any original process.
        if usize::from(snapshot.area.width) / self.columns < 12
            || usize::from(snapshot.area.height) / self.rows < 5
        {
            bail!("Grid is too small for this terminal (minimum 12 columns × 5 lines per pane)");
        }
        Ok(count - snapshot.panes.len())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Pane {
    pub pane_id: String,
    pub rect: Rect,
}

/// Immutable preview inventory. Focus is anchored to the invocation's source,
/// while geometry and membership are compared again immediately before apply.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Snapshot {
    pub workspace_id: String,
    pub tab_id: String,
    pub focused_pane_id: String,
    pub zoomed: bool,
    pub area: Rect,
    pub panes: Vec<Pane>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied { created: usize },
    Refresh(Snapshot),
}

/// Owns the source identity, cwd, and lock for one grid invocation. A popup and
/// the direct CLI use the same executor; neither executes a process-replacing layout API.
pub struct GridController {
    herdr: Herdr,
    source: String,
    cwd: PathBuf,
    lock_path: PathBuf,
}

impl GridController {
    pub fn new(herdr: Herdr, source: String, cwd: PathBuf, lock_path: PathBuf) -> Self {
        Self {
            herdr,
            source,
            cwd,
            lock_path,
        }
    }

    pub fn from_environment(pane: Option<String>) -> Result<Self> {
        let herdr = Herdr::from_env();
        let source = match pane {
            Some(pane) => pane,
            None => crate::context::FocusedPane::from_env()?.pane_id,
        };
        let response: Value = herdr.json(&["pane", "get", &source])?;
        let pane = &response["result"]["pane"];
        let cwd = pane["foreground_cwd"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| pane["cwd"].as_str().filter(|s| !s.is_empty()))
            .context("Source pane has no working directory")?;
        // Key the lock by the actual server socket, including named sessions.
        // Menu-hosted dialogs inherit the menu's state directory, so that
        // variable cannot safely identify a lock shared with standalone grids.
        let socket = std::env::var_os("HERDR_SOCKET_PATH")
            .map(PathBuf::from)
            .context("HERDR_SOCKET_PATH is not set")?;
        Ok(Self::new(
            herdr,
            source,
            PathBuf::from(cwd),
            socket.with_extension("layouts.lock"),
        ))
    }

    pub fn capture(&self) -> Result<Snapshot> {
        let mut snapshot = self.layout_for(&self.source)?;
        snapshot.focused_pane_id = self.source.clone();
        Ok(snapshot)
    }

    pub fn apply(&self, preview: &Snapshot, size: GridSize) -> Result<ApplyOutcome> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&self.lock_path)?;
        // Advisory locks release on process exit, including interrupted popups.
        fs4::FileExt::try_lock(&lock).context("Another grid operation is running")?;
        let current = self.capture()?;
        if current.tab_id != preview.tab_id || current.workspace_id != preview.workspace_id {
            bail!("Source pane moved to another tab; reopen Create layout there");
        }
        if &current != preview {
            return Ok(ApplyOutcome::Refresh(current));
        }
        let created = size.validate(&current)?;
        fs::metadata(&self.cwd)
            .with_context(|| format!("Source directory {} is unavailable", self.cwd.display()))?;
        if current.zoomed {
            self.herdr
                .output(&["pane", "zoom", "--pane", &self.source, "--off"])?;
        }
        if size.count()? == 1 {
            self.restore_focus(&current.tab_id)?;
            return Ok(ApplyOutcome::Applied { created });
        }

        // A one-pane tab needs only splits; it needs no holding tab.
        if current.panes.len() == 1 {
            self.rebuild(&current, size, None).with_context(|| format!(
                "Source {}\nGrid creation stopped. Existing panes remain running; inspect the tab before retrying.", current.tab_id
            ))?;
        } else {
            let label = format!(
                "layouts-grid-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            );
            let cwd = self.cwd.to_str().context("Source cwd is not UTF-8")?;
            let response: Value = self.herdr.json(&[
                "tab", "create", "--workspace", &current.workspace_id, "--cwd", cwd,
                "--label", &label, "--no-focus",
            ]).with_context(|| format!("Creating holding tab {label}; inspect that label if creation was interrupted"))?;
            let tab = response["result"]["tab"]["tab_id"]
                .as_str()
                .filter(|id| !id.is_empty() && *id != current.tab_id)
                .with_context(|| format!("Holding tab {label} returned an invalid ID"))?;
            let root = response["result"]["root_pane"]["pane_id"]
                .as_str()
                .filter(|id| {
                    !id.is_empty() && !current.panes.iter().any(|pane| pane.pane_id == *id)
                })
                .with_context(|| format!("Holding tab {tab} returned an invalid shell ID"))?;
            self.rebuild(&current, size, Some((tab, root))).with_context(|| format!(
                "Source {}\nHolding {tab}\nGrid creation stopped. Original panes remain running.\nInspect both tabs before retrying. Holding label: {label}", current.tab_id
            ))?;
        }
        Ok(ApplyOutcome::Applied { created })
    }

    fn rebuild(
        &self,
        snapshot: &Snapshot,
        size: GridSize,
        holding: Option<(&str, &str)>,
    ) -> Result<()> {
        let count = size.count()?;
        let mut ids = snapshot
            .panes
            .iter()
            .map(|pane| Some(pane.pane_id.clone()))
            .collect::<Vec<_>>();
        if let Some((tab, root)) = holding {
            let mut staged = vec![Some(root.to_owned())];
            staged.extend(ids.iter().skip(1).cloned());
            let tree = SplitTree::grid(0, staged.len(), 1);
            self.fill(
                &tree,
                &mut staged,
                &snapshot.tab_id,
                tab,
                &snapshot.workspace_id,
            )?;
        }
        ids.resize(count, None);
        self.fill(
            &SplitTree::grid(0, size.columns, size.rows),
            &mut ids,
            holding.map(|(tab, _)| tab).unwrap_or(""),
            &snapshot.tab_id,
            &snapshot.workspace_id,
        )?;

        let restored = self.layout_for(ids[0].as_deref().context("Grid lost its root")?)?;
        let expected = ids
            .iter()
            .flatten()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let actual = restored
            .panes
            .iter()
            .map(|pane| pane.pane_id.as_str())
            .collect::<HashSet<_>>();
        if restored.tab_id != snapshot.tab_id
            || restored.workspace_id != snapshot.workspace_id
            || expected.len() != count
            || actual != expected
        {
            bail!("Source membership changed; leaving the holding tab intact");
        }
        if let Some((tab, root)) = holding {
            let remaining = self.layout_for(root)?;
            if remaining.tab_id != tab
                || remaining.panes.len() != 1
                || remaining.panes[0].pane_id != root
            {
                bail!("Holding tab still contains other panes; leaving it intact");
            }
            // Close only the shell we own. A whole-tab close could kill a pane
            // another client inserted between the membership check and cleanup.
            self.herdr.output(&["pane", "close", root])?;
        }
        self.restore_focus(&snapshot.tab_id)
    }

    fn fill(
        &self,
        tree: &SplitTree,
        ids: &mut [Option<String>],
        from: &str,
        to: &str,
        workspace: &str,
    ) -> Result<()> {
        let SplitTree::Split {
            direction,
            ratio,
            first,
            second,
        } = tree
        else {
            return Ok(());
        };
        let target = ids[first.first()]
            .as_deref()
            .context("Missing grid anchor")?
            .to_owned();
        let destination = self.layout_for(&target)?;
        if destination.tab_id != to || destination.workspace_id != workspace || destination.zoomed {
            bail!("Destination for pane {target} changed during grid creation");
        }
        let ratio = ratio.to_string();
        if let Some(pane) = &ids[second.first()] {
            let source = self.layout_for(pane)?;
            if source.tab_id != from || source.workspace_id != workspace || source.zoomed {
                bail!("Pane {pane} moved during grid creation");
            }
            let response: Value = self.herdr.json(&[
                "pane",
                "move",
                pane,
                "--tab",
                to,
                "--target-pane",
                &target,
                "--split",
                direction,
                "--ratio",
                &ratio,
                "--no-focus",
            ])?;
            let moved = &response["result"]["move_result"];
            if moved["changed"].as_bool() != Some(true)
                || moved["pane"]["pane_id"].as_str() != Some(pane)
            {
                bail!("Herdr did not move {pane} with its original identity: {moved}");
            }
        } else {
            let cwd = self.cwd.to_str().context("Source cwd is not UTF-8")?;
            let response: Value = self.herdr.json(&[
                "pane",
                "split",
                &target,
                "--direction",
                direction,
                "--ratio",
                &ratio,
                "--cwd",
                cwd,
                "--no-focus",
            ])?;
            let new_id = response["result"]["pane"]["pane_id"]
                .as_str()
                .filter(|id| !id.is_empty() && !ids.iter().flatten().any(|existing| existing == id))
                .context("Herdr returned an invalid new shell ID")?;
            ids[second.first()] = Some(new_id.to_owned());
        }
        self.fill(first, ids, from, to, workspace)?;
        self.fill(second, ids, from, to, workspace)
    }

    fn layout_for(&self, pane: &str) -> Result<Snapshot> {
        let response: Value = self.herdr.json(&["pane", "layout", "--pane", pane])?;
        let mut snapshot: Snapshot = serde_json::from_value(response["result"]["layout"].clone())
            .context("Invalid pane layout response")?;
        let mut ids = HashSet::new();
        if snapshot.tab_id.is_empty()
            || snapshot.workspace_id.is_empty()
            || snapshot
                .panes
                .iter()
                .any(|p| p.pane_id.is_empty() || !ids.insert(p.pane_id.as_str()))
            || !ids.contains(pane)
        {
            bail!("Herdr returned an invalid layout for pane {pane}");
        }
        snapshot.panes.sort_by(|a, b| {
            (a.rect.y, a.rect.x, &a.pane_id).cmp(&(b.rect.y, b.rect.x, &b.pane_id))
        });
        Ok(snapshot)
    }

    fn restore_focus(&self, tab: &str) -> Result<()> {
        // pane.zoom --off also focuses ordinary shells; agent.focus does not.
        self.herdr
            .output(&["pane", "zoom", "--pane", &self.source, "--off"])?;
        self.herdr.output(&["tab", "focus", tab])?;
        Ok(())
    }
}

/// Balanced insertion schedule. Leaf indices are row-major cells; existing IDs
/// occupy the first cells, and None cells become shells as the tree is filled.
/// Balanced splits keep every ratio inside Herdr's 0.1..0.9 limits.
enum SplitTree {
    Cell(usize),
    Split {
        direction: &'static str,
        ratio: f64,
        first: Box<Self>,
        second: Box<Self>,
    },
}

impl SplitTree {
    fn grid(start: usize, columns: usize, rows: usize) -> Self {
        if rows > 1 {
            let middle = rows.div_ceil(2);
            Self::Split {
                direction: "down",
                ratio: middle as f64 / rows as f64,
                first: Box::new(Self::grid(start, columns, middle)),
                second: Box::new(Self::grid(start + columns * middle, columns, rows - middle)),
            }
        } else if columns > 1 {
            let middle = columns.div_ceil(2);
            Self::Split {
                direction: "right",
                ratio: middle as f64 / columns as f64,
                first: Box::new(Self::grid(start, middle, 1)),
                second: Box::new(Self::grid(start + middle, columns - middle, 1)),
            }
        } else {
            Self::Cell(start)
        }
    }

    fn first(&self) -> usize {
        match self {
            Self::Cell(index) => *index,
            Self::Split { first, .. } => first.first(),
        }
    }
}
