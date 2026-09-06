use crate::cli::OcrArgs;
use crate::config::OcrOptions;
use crate::ocr;

pub fn run(args: &OcrArgs) -> anyhow::Result<()> {
    let options = OcrOptions {
        ocr: args.mode.into(),
        ocr_lang: args.lang.clone(),
        ocr_cache_dir: args.cache_dir.clone(),
        ocr_timeout_secs: args.timeout_secs,
        ocr_command: args.command.clone(),
    };

    let decision = ocr::write_searchable_pdf(&args.input, &args.output, &options, args.verbose)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&decision)?);
    } else if args.verbose {
        eprintln!("  ocr: wrote {}", args.output.display());
    }

    Ok(())
}
