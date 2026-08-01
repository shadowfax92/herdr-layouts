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
    }
}
