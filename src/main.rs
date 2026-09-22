use anyhow::Result;
use clap::{Parser, Subcommand};
use herdr_layouts::Side;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Split {
        #[arg(value_enum)]
        side: Side,
        #[arg(long, default_value_t = 30)]
        percent: u8,
    },
    Equalize,
    /// Arrange existing panes and add empty shells to fill columns x rows.
    Grid {
        size: herdr_layouts::grid::GridSize,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Open the native Create layout popup.
    OpenGrid,
    /// Run the layout dialog in the current terminal (also used by Herdr Menu).
    GridDialog {
        #[arg(long)]
        pane: Option<String>,
    },
    /// Add Create layout to the existing Alt-q menu, with a config backup.
    InstallMenu,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("herdr-layouts: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Split { side, percent } => herdr_layouts::split(side, percent),
        Command::Equalize => herdr_layouts::equalize(),
        Command::OpenGrid => herdr_layouts::grid_ui::open(),
        Command::GridDialog { pane } => herdr_layouts::grid_ui::run(pane),
        Command::InstallMenu => herdr_layouts::install::install_menu(),
        Command::Grid { size, pane } => {
            use herdr_layouts::grid::{ApplyOutcome, GridController};
            let controller = GridController::from_environment(pane)?;
            let snapshot = controller.capture()?;
            match controller.apply(&snapshot, size)? {
                ApplyOutcome::Applied { created } => {
                    println!(
                        "Created {}x{} layout: kept {} panes, added {created} empty shells.",
                        size.columns,
                        size.rows,
                        snapshot.panes.len()
                    );
                    Ok(())
                }
                ApplyOutcome::Refresh(_) => {
                    anyhow::bail!("Layout changed before apply; run the command again")
                }
            }
        }
    }
}
