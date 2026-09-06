use pdf_processor::formula::ocr::{
    normalise_latex, FormulaSidecar, FormulaSidecarStatus, SubprocessSidecar,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pdfp"))
}

fn fixture(name: &str) -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("example")
        .join("pdf")
        .join(name);
    if !path.exists() {
        eprintln!("SKIP: fixture missing {}", path.display());
        return None;
    }
    Some(path)
}

fn unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn temp_crop(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{name}-{}.png", unique_suffix()));
    fs::write(&path, b"dummy").unwrap();
    path
}

fn fake_sidecar_script(name: &str, latex: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{name}-{}", unique_suffix()));
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake-sidecar");
    let mut file = fs::File::create(&script).unwrap();
    writeln!(file, "#!/bin/sh").unwrap();
    writeln!(file, "printf '{}\\n'", latex).unwrap();
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }

    script
}

fn fake_shell_script(name: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{name}-{}", unique_suffix()));
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake-sidecar");
    let mut file = fs::File::create(&script).unwrap();
    writeln!(file, "#!/bin/sh").unwrap();
    writeln!(file, "{body}").unwrap();
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }

    script
}

#[test]
fn subprocess_sidecar_returns_none_when_command_fails() {
    let sidecar = SubprocessSidecar::new("false".to_string());
    let crop = temp_crop("pdfp-formula-ocr-fails");

    let result = sidecar.recognize(&crop);

    assert_eq!(result.status, FormulaSidecarStatus::CommandFailed);
    assert!(
        result.latex.is_none(),
        "failed command should return no LaTeX"
    );
}

#[test]
fn subprocess_sidecar_captures_stdout_as_latex() {
    let sidecar = SubprocessSidecar::new("echo".to_string());
    let crop = temp_crop("pdfp-formula-ocr-echo");

    let result = sidecar.recognize(&crop);

    assert_eq!(result.status, FormulaSidecarStatus::Recovered);
    assert!(result.latex.is_some(), "echo should return stdout content");
}

#[test]
fn subprocess_sidecar_records_empty_output() {
    let script = fake_shell_script("pdfp-formula-ocr-empty", "printf '\\n'");
    let crop = temp_crop("pdfp-formula-ocr-empty-crop");
    let sidecar = SubprocessSidecar::new(script.display().to_string());

    let mut result = sidecar.recognize(&crop);
    for _ in 0..5 {
        if result.status == FormulaSidecarStatus::EmptyOutput {
            break;
        }
        // Brief backoff: freshly-written scripts can hit ETXTBSY on
        // overlayfs-backed tmpfs until the write handle fully drains.
        std::thread::sleep(Duration::from_millis(50));
        result = sidecar.recognize(&crop);
    }

    assert_eq!(
        result.status,
        FormulaSidecarStatus::EmptyOutput,
        "{result:?}"
    );
    assert!(result.latex.is_none());
}

#[test]
fn subprocess_sidecar_records_timeout() {
    let script = fake_shell_script("pdfp-formula-ocr-timeout", "sleep 2");
    let crop = temp_crop("pdfp-formula-ocr-timeout-crop");
    let sidecar =
        SubprocessSidecar::with_timeout(script.display().to_string(), Duration::from_millis(50));

    let mut result = sidecar.recognize(&crop);
    for _ in 0..3 {
        if result.status == FormulaSidecarStatus::Timeout {
            break;
        }
        result = sidecar.recognize(&crop);
    }

    assert_eq!(result.status, FormulaSidecarStatus::Timeout, "{result:?}");
    assert!(result
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("timed out"));
}

#[test]
fn subprocess_sidecar_trims_whitespace_from_output() {
    assert_eq!(normalise_latex("  \\frac{a}{b}  \n"), "\\frac{a}{b}");
}

#[test]
fn subprocess_sidecar_accepts_executable_script_path() {
    let script = fake_sidecar_script("pdfp-formula-ocr-script", "E = mc^2");
    let crop = temp_crop("pdfp-formula-ocr-script-crop");
    let sidecar = SubprocessSidecar::new(script.display().to_string());

    let mut result = sidecar.recognize(&crop);
    for _ in 0..3 {
        if result.latex.as_deref() == Some("E = mc^2") {
            break;
        }
        result = sidecar.recognize(&crop);
    }
    assert_eq!(result.latex.as_deref(), Some("E = mc^2"), "{result:?}");
}

#[test]
fn broken_formula_sidecar_still_writes_markdown_and_debug_index() {
    let Some(pdf) = fixture("math-number-theory.pdf") else {
        return;
    };
    let out = std::env::temp_dir().join(format!(
        "pdfp-formula-sidecar-broken-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);

    let output = Command::new(bin_path())
        .args([
            "convert",
            pdf.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--no-images",
            "--debug-formulas",
            "--formula-sidecar",
            "false",
        ])
        .output()
        .expect("run pdfp convert");

    assert!(
        output.status.success(),
        "convert should not fail when formula sidecar fails\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(out.join("math-number-theory.md").exists());
    let index = fs::read_to_string(out.join("debug/formulas/index.json"))
        .expect("expected formula index despite sidecar failure");
    assert!(index.contains("command-failed"));
}

#[test]
fn convert_with_formula_sidecar_records_recovered_latex_in_debug_json() {
    let Some(pdf) = fixture("math-number-theory.pdf") else {
        return;
    };
    let out = std::env::temp_dir().join(format!("pdfp-formula-sidecar-e2e-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let script = fake_sidecar_script("pdfp-formula-sidecar-e2e-script", "E = mc^2");

    let output = Command::new(bin_path())
        .args([
            "convert",
            pdf.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--no-images",
            "--debug-formulas",
            "--formula-sidecar",
            script.to_str().unwrap(),
        ])
        .output()
        .expect("run pdfp convert");

    assert!(
        output.status.success(),
        "convert failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let debug_dir = out.join("debug").join("formulas");
    let mut debug_json = String::new();
    for entry in fs::read_dir(&debug_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "json") {
            debug_json.push_str(&fs::read_to_string(path).unwrap());
        }
    }

    assert!(
        debug_json.contains("\"latex\": \"E = mc^2\""),
        "expected sidecar latex in formula debug JSON"
    );
    assert!(
        debug_json.contains("\"backend\": \"formula-sidecar\""),
        "expected sidecar backend in formula debug JSON"
    );
}
