use std::collections::hash_map::DefaultHasher;
use std::ffi::OsStr;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context};
use serde::Serialize;

use crate::config::{OcrMode, OcrOptions};
use crate::pdf::extractor::PdfExtractor;

pub mod triage;

const DEFAULT_OCR_COMMAND: &str = "ocrmypdf";
const OCR_COMMAND_ENV: &str = "PDFP_OCR_COMMAND";

#[derive(Debug, Clone)]
pub struct PreparedPdf {
    pub effective_path: PathBuf,
    pub decision: OcrDecision,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrDecision {
    pub mode: String,
    pub status: OcrStatus,
    pub provider: String,
    pub language: String,
    pub command: String,
    pub input: String,
    pub output: Option<String>,
    pub cache_hit: bool,
    pub pages_needing_ocr: Vec<usize>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrRuntimeStatus {
    pub available: bool,
    pub command: Option<String>,
    pub source: Option<String>,
    pub searched: Vec<String>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OcrCommandResolution {
    pub command: PathBuf,
    pub source: OcrCommandSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrCommandSource {
    Explicit,
    Env,
    Bundled,
    Path,
}

impl OcrCommandSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Env => "env",
            Self::Bundled => "bundled",
            Self::Path => "path",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OcrStatus {
    Off,
    Skipped,
    CacheHit,
    Ran,
}

impl OcrDecision {
    pub fn off(input: &Path, options: &OcrOptions) -> Self {
        Self {
            mode: "off".to_string(),
            status: OcrStatus::Off,
            provider: "ocrmypdf".to_string(),
            language: options.ocr_lang.clone(),
            command: options.ocr_command.display().to_string(),
            input: input.display().to_string(),
            output: None,
            cache_hit: false,
            pages_needing_ocr: Vec::new(),
            reason: "ocr disabled".to_string(),
        }
    }

    fn skipped(input: &Path, options: &OcrOptions, reason: impl Into<String>) -> Self {
        Self {
            mode: mode_name(options.ocr).to_string(),
            status: OcrStatus::Skipped,
            provider: "ocrmypdf".to_string(),
            language: options.ocr_lang.clone(),
            command: options.ocr_command.display().to_string(),
            input: input.display().to_string(),
            output: None,
            cache_hit: false,
            pages_needing_ocr: Vec::new(),
            reason: reason.into(),
        }
    }
}

pub fn prepare_pdf(
    input: &Path,
    options: &OcrOptions,
    verbose: bool,
) -> anyhow::Result<PreparedPdf> {
    if matches!(options.ocr, OcrMode::Off) {
        return Ok(PreparedPdf {
            effective_path: input.to_path_buf(),
            decision: OcrDecision::off(input, options),
        });
    }

    let (raw_pages, _) = PdfExtractor::extract(input)
        .with_context(|| format!("Failed to inspect {}", input.display()))?;
    let report = triage::triage_raw_pages(&raw_pages);
    prepare_pdf_with_report(input, options, &report, verbose)
}

pub fn prepare_pdf_with_report(
    input: &Path,
    options: &OcrOptions,
    report: &triage::OcrTriageReport,
    verbose: bool,
) -> anyhow::Result<PreparedPdf> {
    if matches!(options.ocr, OcrMode::Off) {
        return Ok(PreparedPdf {
            effective_path: input.to_path_buf(),
            decision: OcrDecision::off(input, options),
        });
    }

    let needs_ocr = match options.ocr {
        OcrMode::Off => false,
        OcrMode::Auto => !report.pages_needing_ocr.is_empty(),
        OcrMode::Force => true,
    };

    if !needs_ocr {
        if verbose {
            eprintln!("  ocr: skipped; PDF has readable text");
        }
        return Ok(PreparedPdf {
            effective_path: input.to_path_buf(),
            decision: OcrDecision::skipped(input, options, "pdf has readable text"),
        });
    }

    let output = ocr_output_path(input, options)?;
    let key_reason = if options.ocr_cache_dir.is_some() {
        "cache key matched"
    } else {
        "temporary OCR output"
    };

    if output.exists() {
        if verbose {
            eprintln!("  ocr: cache hit {}", output.display());
        }
        return Ok(PreparedPdf {
            effective_path: output.clone(),
            decision: OcrDecision {
                mode: mode_name(options.ocr).to_string(),
                status: OcrStatus::CacheHit,
                provider: "ocrmypdf".to_string(),
                language: options.ocr_lang.clone(),
                command: options.ocr_command.display().to_string(),
                input: input.display().to_string(),
                output: Some(output.display().to_string()),
                cache_hit: true,
                pages_needing_ocr: report.pages_needing_ocr.clone(),
                reason: key_reason.to_string(),
            },
        });
    }

    let resolution =
        resolve_ocr_command(options).map_err(|_| missing_ocr_error(options, "--ocr off"))?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create OCR cache dir {}", parent.display()))?;
    }

    if verbose {
        eprintln!(
            "  ocr: running {} on {} -> {}",
            resolution.command.display(),
            input.display(),
            output.display()
        );
    }

    run_ocrmypdf(input, &output, options, &resolution.command)?;

    Ok(PreparedPdf {
        effective_path: output.clone(),
        decision: OcrDecision {
            mode: mode_name(options.ocr).to_string(),
            status: OcrStatus::Ran,
            provider: "ocrmypdf".to_string(),
            language: options.ocr_lang.clone(),
            command: resolution.command.display().to_string(),
            input: input.display().to_string(),
            output: Some(output.display().to_string()),
            cache_hit: false,
            pages_needing_ocr: report.pages_needing_ocr.clone(),
            reason: "ocr completed".to_string(),
        },
    })
}

pub fn write_searchable_pdf(
    input: &Path,
    output: &Path,
    options: &OcrOptions,
    verbose: bool,
) -> anyhow::Result<OcrDecision> {
    ensure_output_is_not_input(input, output)?;

    let (raw_pages, _) = PdfExtractor::extract(input)
        .with_context(|| format!("Failed to inspect {}", input.display()))?;
    let report = triage::triage_raw_pages(&raw_pages);

    let needs_ocr = match options.ocr {
        OcrMode::Off => false,
        OcrMode::Auto => !report.pages_needing_ocr.is_empty(),
        OcrMode::Force => true,
    };

    if !needs_ocr {
        copy_pdf(input, output)?;
        if verbose {
            eprintln!(
                "  ocr: skipped; copied readable PDF to {}",
                output.display()
            );
        }
        let mut decision =
            OcrDecision::skipped(input, options, "pdf has readable text; copied input");
        decision.output = Some(output.display().to_string());
        return Ok(decision);
    }

    if options.ocr_cache_dir.is_some() {
        let prepared = prepare_pdf_with_report(input, options, &report, verbose)?;
        if prepared.effective_path != output {
            copy_pdf(&prepared.effective_path, output)?;
        }
        let mut decision = prepared.decision;
        decision.output = Some(output.display().to_string());
        return Ok(decision);
    }

    let resolution = resolve_ocr_command(options)
        .map_err(|_| missing_ocr_error(options, "--mode auto on a readable PDF"))?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create OCR output dir {}", parent.display()))?;
    }

    if verbose {
        eprintln!(
            "  ocr: running {} on {} -> {}",
            resolution.command.display(),
            input.display(),
            output.display()
        );
    }

    run_ocrmypdf(input, output, options, &resolution.command)?;

    Ok(OcrDecision {
        mode: mode_name(options.ocr).to_string(),
        status: OcrStatus::Ran,
        provider: "ocrmypdf".to_string(),
        language: options.ocr_lang.clone(),
        command: resolution.command.display().to_string(),
        input: input.display().to_string(),
        output: Some(output.display().to_string()),
        cache_hit: false,
        pages_needing_ocr: report.pages_needing_ocr,
        reason: "ocr completed".to_string(),
    })
}

fn run_ocrmypdf(
    input: &Path,
    output: &Path,
    options: &OcrOptions,
    command_path: &Path,
) -> anyhow::Result<()> {
    let mut command = Command::new(command_path);
    command
        .arg("--language")
        .arg(&options.ocr_lang)
        .arg("--output-type")
        .arg("pdf");
    match options.ocr {
        OcrMode::Auto => {
            command.arg("--skip-text");
        }
        OcrMode::Force => {
            command.arg("--force-ocr");
        }
        OcrMode::Off => {}
    }
    command
        .arg(input)
        .arg(output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start {}", command_path.display()))?;
    let timeout = Duration::from_secs(options.ocr_timeout_secs);
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let output = child.wait_with_output()?;
            if status.success() {
                return Ok(());
            }
            bail!(
                "OCRmyPDF failed with status {status}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "OCRmyPDF timed out after {} seconds for {}",
                options.ocr_timeout_secs,
                input.display()
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn copy_pdf(input: &Path, output: &Path) -> anyhow::Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create OCR output dir {}", parent.display()))?;
    }
    fs::copy(input, output).with_context(|| {
        format!(
            "failed to copy readable PDF {} -> {}",
            input.display(),
            output.display()
        )
    })?;
    Ok(())
}

fn ensure_output_is_not_input(input: &Path, output: &Path) -> anyhow::Result<()> {
    if absolute_path(input)? == absolute_path(output)? {
        bail!("OCR output path must be different from input path");
    }
    if output.exists() && fs::canonicalize(input).ok() == fs::canonicalize(output).ok() {
        bail!("OCR output path must be different from input path");
    }
    Ok(())
}

fn absolute_path(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn ocr_output_path(input: &Path, options: &OcrOptions) -> anyhow::Result<PathBuf> {
    let key = cache_key(input, options)?;
    let stem = input
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("document");
    let filename = format!("{stem}-{key}.ocr.pdf");
    Ok(match &options.ocr_cache_dir {
        Some(dir) => dir.join(filename),
        None => std::env::temp_dir()
            .join(format!("pdfp-ocr-{}", std::process::id()))
            .join(filename),
    })
}

pub fn cache_key(input: &Path, options: &OcrOptions) -> anyhow::Result<String> {
    let metadata =
        fs::metadata(input).with_context(|| format!("failed to stat {}", input.display()))?;
    let mut hasher = DefaultHasher::new();
    input.display().to_string().hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    modified_secs(&metadata).hash(&mut hasher);
    mode_name(options.ocr).hash(&mut hasher);
    options.ocr_lang.hash(&mut hasher);
    options.ocr_command.display().to_string().hash(&mut hasher);
    options.ocr_timeout_secs.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn modified_secs(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn ocr_runtime_status(options: &OcrOptions) -> OcrRuntimeStatus {
    let searched = ocr_search_paths(options)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect();
    match resolve_ocr_command(options) {
        Ok(resolution) => OcrRuntimeStatus {
            available: true,
            command: Some(resolution.command.display().to_string()),
            source: Some(resolution.source.as_str().to_string()),
            searched,
            hint: None,
        },
        Err(_) => OcrRuntimeStatus {
            available: false,
            command: None,
            source: None,
            searched,
            hint: Some(ocr_install_hint().to_string()),
        },
    }
}

pub fn resolve_ocr_command(options: &OcrOptions) -> anyhow::Result<OcrCommandResolution> {
    let requested = &options.ocr_command;
    let default_requested = requested == Path::new(DEFAULT_OCR_COMMAND);

    if !default_requested {
        return resolve_named_or_path_command(requested, OcrCommandSource::Explicit);
    }

    if let Some(env_command) = std::env::var_os(OCR_COMMAND_ENV).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(env_command);
        return resolve_named_or_path_command(&path, OcrCommandSource::Env);
    }

    for candidate in bundled_ocr_candidates() {
        if is_executable_candidate(&candidate) {
            return Ok(OcrCommandResolution {
                command: candidate,
                source: OcrCommandSource::Bundled,
            });
        }
    }

    resolve_named_or_path_command(requested, OcrCommandSource::Path)
}

fn resolve_named_or_path_command(
    command: &Path,
    source: OcrCommandSource,
) -> anyhow::Result<OcrCommandResolution> {
    if has_path_component(command) {
        if is_executable_candidate(command) {
            return Ok(OcrCommandResolution {
                command: command.to_path_buf(),
                source,
            });
        }
        bail!("{} was not found", command.display());
    }

    let Some(paths) = std::env::var_os("PATH") else {
        bail!("PATH is not set");
    };

    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(command);
        if is_executable_candidate(&candidate) {
            return Ok(OcrCommandResolution {
                command: candidate,
                source,
            });
        }
    }

    bail!("{} was not found on PATH", command.display())
}

fn ocr_search_paths(options: &OcrOptions) -> Vec<PathBuf> {
    let requested = &options.ocr_command;
    if requested != Path::new(DEFAULT_OCR_COMMAND) {
        return vec![requested.clone()];
    }

    let mut paths = Vec::new();
    if let Some(env_command) = std::env::var_os(OCR_COMMAND_ENV).filter(|value| !value.is_empty()) {
        paths.push(PathBuf::from(env_command));
    }
    paths.extend(bundled_ocr_candidates());
    if let Some(path_var) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path_var).map(|dir| dir.join(DEFAULT_OCR_COMMAND)));
    }
    paths
}

fn bundled_ocr_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("tools").join("ocr").join(ocr_binary_name()));
            candidates.push(
                dir.parent()
                    .unwrap_or(dir)
                    .join("tools")
                    .join("ocr")
                    .join(ocr_binary_name()),
            );
        }
    }
    candidates
}

fn has_path_component(command: &Path) -> bool {
    command.components().count() > 1
}

fn is_executable_candidate(path: &Path) -> bool {
    path.is_file()
}

fn ocr_binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "ocrmypdf.exe"
    }
    #[cfg(not(windows))]
    {
        DEFAULT_OCR_COMMAND
    }
}

fn missing_ocr_error(options: &OcrOptions, fallback: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "OCR requested but OCRmyPDF command `{}` was not found. {}. Rerun with `{}` if OCR is not required.",
        options.ocr_command.display(),
        ocr_install_hint(),
        fallback
    )
}

pub fn ocr_install_hint() -> &'static str {
    "Install the full pdfp release bundle, set PDFP_OCR_COMMAND, or install OCRmyPDF/Tesseract with your package manager"
}

fn mode_name(mode: OcrMode) -> &'static str {
    match mode {
        OcrMode::Off => "off",
        OcrMode::Auto => "auto",
        OcrMode::Force => "force",
    }
}

#[cfg(test)]
fn command_args_for_test(input: &Path, output: &Path, options: &OcrOptions) -> Vec<String> {
    let mut args = vec![
        "--language".to_string(),
        options.ocr_lang.clone(),
        "--output-type".to_string(),
        "pdf".to_string(),
    ];
    match options.ocr {
        OcrMode::Auto => args.push("--skip-text".to_string()),
        OcrMode::Force => args.push("--force-ocr".to_string()),
        OcrMode::Off => {}
    }
    args.push(input.display().to_string());
    args.push(output.display().to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(mode: OcrMode) -> OcrOptions {
        OcrOptions {
            ocr: mode,
            ocr_lang: "eng".to_string(),
            ocr_cache_dir: None,
            ocr_timeout_secs: 600,
            ocr_command: PathBuf::from("ocrmypdf"),
        }
    }

    #[test]
    fn command_args_use_skip_text_for_auto() {
        let args = command_args_for_test(
            Path::new("in.pdf"),
            Path::new("out.pdf"),
            &options(OcrMode::Auto),
        );
        assert!(args.contains(&"--skip-text".to_string()));
        assert!(!args.contains(&"--force-ocr".to_string()));
    }

    #[test]
    fn command_args_use_force_ocr_for_force() {
        let args = command_args_for_test(
            Path::new("in.pdf"),
            Path::new("out.pdf"),
            &options(OcrMode::Force),
        );
        assert!(args.contains(&"--force-ocr".to_string()));
        assert!(!args.contains(&"--skip-text".to_string()));
    }

    #[test]
    fn cache_key_changes_with_language() {
        let input =
            std::env::temp_dir().join(format!("pdfp-cache-key-test-{}.pdf", std::process::id()));
        std::fs::write(&input, b"%PDF-1.4\n").unwrap();

        let a = options(OcrMode::Auto);
        let mut b = options(OcrMode::Auto);
        b.ocr_lang = "eng+deu".to_string();

        let base = cache_key(&input, &a).unwrap();
        assert_ne!(base, cache_key(&input, &b).unwrap());

        let mut c = options(OcrMode::Auto);
        c.ocr_command = PathBuf::from("other-ocrmypdf");
        assert_ne!(base, cache_key(&input, &c).unwrap());

        let _ = std::fs::remove_file(input);
    }
}
