use serde::Serialize;

use crate::cli::DoctorArgs;
use crate::ocr;

#[derive(Debug, Serialize)]
struct DoctorReport {
    pdfp: PdfpStatus,
    ocr: ocr::OcrRuntimeStatus,
}

#[derive(Debug, Serialize)]
struct PdfpStatus {
    version: &'static str,
}

pub fn run(args: &DoctorArgs) -> anyhow::Result<()> {
    let report = DoctorReport {
        pdfp: PdfpStatus {
            version: env!("CARGO_PKG_VERSION"),
        },
        ocr: ocr::ocr_runtime_status(&crate::config::OcrOptions::default()),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("pdfp {}", report.pdfp.version);
        if report.ocr.available {
            println!(
                "ocr: available via {} ({})",
                report.ocr.command.as_deref().unwrap_or("unknown"),
                report.ocr.source.as_deref().unwrap_or("unknown")
            );
        } else {
            println!("ocr: unavailable");
            if let Some(hint) = &report.ocr.hint {
                println!("hint: {hint}");
            }
        }
    }

    Ok(())
}
