use crate::batch::{self, is_pdf};
use std::path::Path;

use anyhow::Context;

use crate::cli::{AppCommand, ConvertArgs};
use crate::config;
use crate::pipeline;
use crate::processor;

pub fn run(command: AppCommand) -> anyhow::Result<()> {
    match command {
        AppCommand::Convert(args) => run_convert(args),
        AppCommand::Ocr(args) => processor::ocr_cmd::run(&args),
        AppCommand::Doctor(args) => processor::doctor::run(&args),
        AppCommand::Inspect(args) => processor::inspect::run(&args),
        AppCommand::Metadata(args) => processor::metadata::run(&args),
        AppCommand::Search(args) => processor::search::run(&args),
        AppCommand::Eval(args) => run_eval(args),
        AppCommand::Pages(args) => processor::pages::run(&args),
        AppCommand::Impose(args) => processor::impose::run(&args),
        AppCommand::Page(args) => processor::resize::run(&args),
        AppCommand::Update(args) => processor::update::run(&args),
    }
}

fn run_eval(args: crate::cli::EvalArgs) -> anyhow::Result<()> {
    anyhow::ensure!(
        args.dir.exists(),
        "fixture dir does not exist: {}",
        args.dir.display()
    );
    anyhow::ensure!(
        args.dir.is_dir(),
        "fixture path is not a directory: {}",
        args.dir.display()
    );

    let fixtures = crate::eval::fixtures::load_fixtures(&args.dir)?;
    if fixtures.is_empty() {
        println!("no fixture files found in {}", args.dir.display());
        return Ok(());
    }

    let results = crate::eval::runner::run_eval(&fixtures);
    let mut skipped = 0usize;
    for result in &results {
        if let Some(error) = &result.error {
            eprintln!("eval: skipped {}: {error}", result.doc_name);
            skipped += 1;
            continue;
        }
        crate::eval::metrics::print_report(&result.doc_name, &result.metrics);
    }
    println!(
        "evaluated {} document(s), skipped {}",
        results.len().saturating_sub(skipped),
        skipped
    );
    Ok(())
}

fn run_convert(args: ConvertArgs) -> anyhow::Result<()> {
    let inputs = batch::resolve_inputs(&args.input)
        .with_context(|| format!("Failed to resolve input '{}'", args.input))?;

    let mut options = args.options.into_config();
    options.batch_mode = inputs.len() > 1;

    if options.verbose {
        eprintln!("Processing {} PDF file(s)", inputs.len());
    }

    let results: Vec<(std::path::PathBuf, anyhow::Result<()>)> = inputs
        .iter()
        .map(|path| (path.clone(), process_one(path, &options)))
        .collect();

    let mut had_errors = false;
    for (path, result) in &results {
        match result {
            Ok(()) => {
                if options.verbose {
                    eprintln!("  ok: {}", path.display());
                }
            }
            Err(e) => {
                eprintln!("  error: {}: {}", path.display(), e);
                had_errors = true;
            }
        }
    }

    if had_errors {
        std::process::exit(1);
    }

    Ok(())
}

fn process_one(path: &Path, options: &config::ConvertOptions) -> anyhow::Result<()> {
    if !is_pdf(path) {
        anyhow::bail!("Unsupported file type: {}", path.display());
    }
    Ok(pipeline::process_pdf(path, options)?)
}
