//! A non-mutating grid preview until Enter, shared by the native plugin popup
//! and a foreground Herdr Menu item. The source pane survives both PTY handoffs.

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

use crate::grid::{ApplyOutcome, GridController, GridSize, Snapshot};
use crate::herdr::Herdr;

pub fn open() -> Result<()> {
    let source = crate::context::FocusedPane::from_env()?;
    // Herdr popups are session-modal and reject --target-pane. Carry the
    // captured source explicitly into the child instead of rereading UI focus.
    Herdr::from_env().output(&[
        "plugin",
        "pane",
        "open",
        "--plugin",
        "shadowfax.layouts",
        "--entrypoint",
        "grid",
        "--env",
        &format!("HERDR_LAYOUTS_SOURCE_PANE_ID={}", source.pane_id),
    ])?;
    Ok(())
}

pub fn run(pane: Option<String>) -> Result<()> {
    let pane = pane.or_else(|| std::env::var("HERDR_LAYOUTS_SOURCE_PANE_ID").ok());
    // Initialize even on source errors, so action failures remain readable in
    // the popup instead of disappearing when its short-lived process exits.
    let controller = GridController::from_environment(pane);
    let snapshot = controller
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error:#}"))
        .and_then(GridController::capture);
    let mut state = match snapshot {
        Ok(snapshot) => Dialog::new(snapshot),
        Err(error) => Dialog::failed(format!("{error:#}")),
    };
    let mut terminal = ratatui::try_init().context("Initializing layout dialog")?;
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|frame| state.render(frame))?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            match state.handle_key(key) {
                Input::Continue => {}
                Input::Close => return Ok(()),
                Input::Apply(size) => {
                    state.message = Some("Creating layout…".into());
                    terminal.draw(|frame| state.render(frame))?;
                    let controller = controller
                        .as_ref()
                        .map_err(|error| anyhow::anyhow!("{error:#}"))?;
                    match controller
                        .apply(state.snapshot.as_ref().context("Missing preview")?, size)
                    {
                        Ok(ApplyOutcome::Applied { .. }) => return Ok(()),
                        Ok(ApplyOutcome::Refresh(snapshot)) => {
                            state.snapshot = Some(snapshot);
                            state.message = Some(
                                "Layout changed. Review the preview, then Enter again.".into(),
                            );
                        }
                        Err(error) => {
                            // A partial move may have changed both tabs. Disable
                            // retries against the old inventory and keep recovery IDs visible.
                            state.failure = Some(format!("{error:#}"));
                            state.scroll = 0;
                        }
                    }
                }
            }
        }
    })();
    let restore = ratatui::try_restore().context("Restoring terminal");
    result?;
    restore
}

#[derive(Debug, PartialEq, Eq)]
pub enum Input {
    Continue,
    Close,
    Apply(GridSize),
}

/// Dialog-local edits never call Herdr. Apply hands one exact snapshot to the
/// controller, which can return a refreshed inventory for another Enter.
pub struct Dialog {
    snapshot: Option<Snapshot>,
    input: String,
    message: Option<String>,
    failure: Option<String>,
    scroll: u16,
}

impl Dialog {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot: Some(snapshot),
            input: String::new(),
            message: None,
            failure: None,
            scroll: 0,
        }
    }

    fn failed(error: String) -> Self {
        Self {
            snapshot: None,
            input: String::new(),
            message: None,
            failure: Some(error),
            scroll: 0,
        }
    }

    fn size(&self) -> Result<(GridSize, usize)> {
        let size: GridSize = self.input.parse()?;
        let created = size.validate(self.snapshot.as_ref().context("No source layout")?)?;
        Ok((size, created))
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Input {
        if key.kind == KeyEventKind::Release {
            return Input::Continue;
        }
        if key.code == KeyCode::Esc
            || (key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('q'))
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
        {
            return Input::Close;
        }
        if self.failure.is_some() {
            match key.code {
                KeyCode::Down => self.scroll = self.scroll.saturating_add(1),
                KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
                KeyCode::PageDown => self.scroll = self.scroll.saturating_add(8),
                KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(8),
                KeyCode::Home => self.scroll = 0,
                _ => {}
            }
            return Input::Continue;
        }
        match key.code {
            KeyCode::Enter if key.kind == KeyEventKind::Press => {
                if let Ok((size, _)) = self.size() {
                    return Input::Apply(size);
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.message = None;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.clear();
                self.message = None;
            }
            KeyCode::Char(c)
                if (c.is_ascii_digit() || matches!(c, 'x' | 'X' | '×' | ' '))
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && self.input.len() < 32 =>
            {
                self.input.push(c);
                self.message = None;
            }
            _ => {}
        }
        Input::Continue
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let block = Block::bordered()
            .title(" Create layout ")
            .border_style(Style::new().fg(Color::Cyan));
        let inner = block.inner(frame.area());
        frame.render_widget(block, frame.area());
        if let Some(failure) = &self.failure {
            let [body, footer] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            let lines = wrap_error(failure, body.width.max(1) as usize);
            let max_scroll = lines.len().saturating_sub(body.height as usize);
            let error = Paragraph::new(lines).style(Style::new().fg(Color::Red));
            self.scroll = self.scroll.min(max_scroll.min(u16::MAX as usize) as u16);
            frame.render_widget(error.scroll((self.scroll, 0)), body);
            frame.render_widget(Paragraph::new("↑/↓ scroll  esc / alt+q close"), footer);
            return;
        }
        let [input, status, preview, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .areas(inner);
        frame.render_widget(
            Paragraph::new(self.input.as_str())
                .block(Block::bordered().title(" Columns x rows · e.g. 5x2 ")),
            input,
        );
        if input.width > 2 && input.height > 2 {
            frame.set_cursor_position((
                input.x + 1 + (self.input.chars().count() as u16).min(input.width - 3),
                input.y + 1,
            ));
        }
        match self.size() {
            Ok((size, created)) => {
                let snapshot = self.snapshot.as_ref().expect("size validated snapshot");
                let text = format!(
                    "{} columns × {} rows · {} panes\nKeep {} existing · add {} empty shells{}",
                    size.columns,
                    size.rows,
                    size.columns * size.rows,
                    snapshot.panes.len(),
                    created,
                    self.message
                        .as_ref()
                        .map(|message| format!("\n{message}"))
                        .unwrap_or_default()
                );
                frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), status);
                render_grid(frame, preview, snapshot, size);
            }
            Err(error) => {
                let text = if self.input.is_empty() {
                    format!(
                        "{} panes in this tab. Enter a grid size.",
                        self.snapshot.as_ref().map_or(0, |s| s.panes.len())
                    )
                } else {
                    error.to_string()
                };
                frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), status);
            }
        }
        frame.render_widget(Paragraph::new("enter create  ctrl+u clear  esc / alt+q cancel\nCyan: focused pane · new: empty shell").style(Style::new().fg(Color::DarkGray)), footer);
    }
}

// Explicit wrapping makes the recovery screen's scroll bounds independent of
// Ratatui's private line-count API and keeps even long paths fully readable.
fn wrap_error(message: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for source in message.lines() {
        let mut line = String::new();
        let mut cells = 0;
        for character in source.chars() {
            let size = Span::raw(character.to_string()).width();
            if cells + size > width && !line.is_empty() {
                lines.push(Line::from(std::mem::take(&mut line)));
                cells = 0;
            }
            line.push(character);
            cells += size;
        }
        lines.push(Line::from(line));
    }
    lines
}

fn render_grid(frame: &mut Frame, area: Rect, snapshot: &Snapshot, size: GridSize) {
    if area.width as usize / size.columns < 4 || area.height as usize / size.rows < 3 {
        frame.render_widget(
            Paragraph::new("Preview needs a larger popup; the grid fits your tab."),
            area,
        );
        return;
    }
    for row in 0..size.rows {
        for col in 0..size.columns {
            let index = row * size.columns + col;
            let pane = snapshot.panes.get(index);
            let focused = pane.is_some_and(|p| p.pane_id == snapshot.focused_pane_id);
            let label = pane.map(|p| p.pane_id.as_str()).unwrap_or("new");
            let x = area.width as usize * col / size.columns;
            let y = area.height as usize * row / size.rows;
            let width = area.width as usize * (col + 1) / size.columns - x;
            let height = area.height as usize * (row + 1) / size.rows - y;
            frame.render_widget(
                Paragraph::new(label)
                    .centered()
                    .block(Block::bordered())
                    .style(Style::new().fg(if focused {
                        Color::Cyan
                    } else if pane.is_none() {
                        Color::Green
                    } else {
                        Color::White
                    })),
                Rect::new(
                    area.x + x as u16,
                    area.y + y as u16,
                    width as u16,
                    height as u16,
                ),
            );
        }
    }
}
