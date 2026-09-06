//! Formula OCR sidecar contract and subprocess implementation.

use serde::Serialize;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Result status for a formula OCR sidecar attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FormulaSidecarStatus {
    NotAttempted,
    Attempted,
    Recovered,
    EmptyOutput,
    Timeout,
    CommandFailed,
    RejectedByPolicy,
}

/// Structured formula OCR sidecar attempt data for debug/audit reports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FormulaSidecarAttempt {
    pub status: FormulaSidecarStatus,
    pub backend: Option<String>,
    pub latex: Option<String>,
    pub duration_ms: Option<u64>,
    pub stderr: Option<String>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sanity: Option<String>,
}

impl FormulaSidecarAttempt {
    pub fn not_attempted() -> Self {
        Self {
            status: FormulaSidecarStatus::NotAttempted,
            backend: None,
            latex: None,
            duration_ms: None,
            stderr: None,
            error: None,
            sanity: None,
        }
    }

    pub fn rejected_by_policy(reason: impl Into<String>) -> Self {
        Self {
            status: FormulaSidecarStatus::RejectedByPolicy,
            backend: None,
            latex: None,
            duration_ms: None,
            stderr: None,
            error: Some(reason.into()),
            sanity: None,
        }
    }
}

/// Converts a formula crop image to a LaTeX string.
pub trait FormulaSidecar: Send + Sync {
    /// Returns a structured recognition attempt. Implementations should not
    /// panic or abort conversion on recognition failure.
    fn recognize(&self, crop_path: &Path) -> FormulaSidecarAttempt;
}

/// Formula OCR sidecar backed by an external command.
///
/// The command receives the crop PNG path as its first positional argument and
/// should print LaTeX to stdout with exit status 0.
pub struct SubprocessSidecar {
    command: String,
    timeout: Duration,
}

impl SubprocessSidecar {
    pub fn new(command: String) -> Self {
        Self::with_timeout(command, Duration::from_secs(30))
    }

    pub fn with_timeout(command: String, timeout: Duration) -> Self {
        Self { command, timeout }
    }
}

impl FormulaSidecar for SubprocessSidecar {
    fn recognize(&self, crop_path: &Path) -> FormulaSidecarAttempt {
        let start = Instant::now();
        let mut child = match Command::new(&self.command)
            .arg(crop_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                return FormulaSidecarAttempt {
                    status: FormulaSidecarStatus::CommandFailed,
                    backend: Some("formula-sidecar".to_string()),
                    latex: None,
                    duration_ms: Some(elapsed_ms(start)),
                    stderr: None,
                    error: Some(err.to_string()),
                    sanity: None,
                };
            }
        };

        loop {
            if start.elapsed() >= self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                let (_stdout, stderr) = collect_child_pipes(&mut child);
                return FormulaSidecarAttempt {
                    status: FormulaSidecarStatus::Timeout,
                    backend: Some("formula-sidecar".to_string()),
                    latex: None,
                    duration_ms: Some(elapsed_ms(start)),
                    stderr: Some(stderr_summary(&stderr)).filter(|stderr| !stderr.is_empty()),
                    error: Some(format!("timed out after {}ms", self.timeout.as_millis())),
                    sanity: None,
                };
            }

            match child.try_wait() {
                Ok(Some(status)) => return finish_subprocess_attempt(status, child, start),
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(err) => {
                    let _ = child.kill();
                    return FormulaSidecarAttempt {
                        status: FormulaSidecarStatus::CommandFailed,
                        backend: Some("formula-sidecar".to_string()),
                        latex: None,
                        duration_ms: Some(elapsed_ms(start)),
                        stderr: None,
                        error: Some(err.to_string()),
                        sanity: None,
                    };
                }
            }
        }
    }
}

fn finish_subprocess_attempt(
    status: ExitStatus,
    mut child: Child,
    start: Instant,
) -> FormulaSidecarAttempt {
    let (stdout_bytes, stderr_bytes) = collect_child_pipes(&mut child);
    if !status.success() {
        return FormulaSidecarAttempt {
            status: FormulaSidecarStatus::CommandFailed,
            backend: Some("formula-sidecar".to_string()),
            latex: None,
            duration_ms: Some(elapsed_ms(start)),
            stderr: Some(stderr_summary(&stderr_bytes)).filter(|stderr| !stderr.is_empty()),
            error: Some(format!("exit status {status}")),
            sanity: None,
        };
    }

    let stdout = match String::from_utf8(stdout_bytes) {
        Ok(stdout) => stdout,
        Err(err) => {
            return FormulaSidecarAttempt {
                status: FormulaSidecarStatus::CommandFailed,
                backend: Some("formula-sidecar".to_string()),
                latex: None,
                duration_ms: Some(elapsed_ms(start)),
                stderr: Some(stderr_summary(&stderr_bytes)).filter(|stderr| !stderr.is_empty()),
                error: Some(err.to_string()),
                sanity: None,
            };
        }
    };
    let latex = normalise_latex(&stdout);
    if latex.is_empty() {
        FormulaSidecarAttempt {
            status: FormulaSidecarStatus::EmptyOutput,
            backend: Some("formula-sidecar".to_string()),
            latex: None,
            duration_ms: Some(elapsed_ms(start)),
            stderr: Some(stderr_summary(&stderr_bytes)).filter(|stderr| !stderr.is_empty()),
            error: None,
            sanity: None,
        }
    } else {
        FormulaSidecarAttempt {
            status: FormulaSidecarStatus::Recovered,
            backend: Some("formula-sidecar".to_string()),
            latex: Some(latex.to_string()),
            duration_ms: Some(elapsed_ms(start)),
            stderr: Some(stderr_summary(&stderr_bytes)).filter(|stderr| !stderr.is_empty()),
            error: None,
            sanity: None,
        }
    }
}

fn collect_child_pipes(child: &mut Child) -> (Vec<u8>, Vec<u8>) {
    let mut stdout = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut stdout);
    }
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut stderr);
    }
    (stdout, stderr)
}

pub fn normalise_latex(raw: &str) -> &str {
    raw.trim()
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn stderr_summary(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines().take(8).collect::<Vec<_>>().join("\n")
}
