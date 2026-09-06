use clap::Parser;

use pdf_processor::cli::Cli;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    pdf_processor::commands::run(cli.into_command()?)
}
