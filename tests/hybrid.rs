//! Integration tests for the hybrid Docling backend.
//!
//! Spins up an in-process `httpmock` server, points the client at it, and
//! asserts on the request shape and returned markdown. A separate
//! `#[ignore]`-tagged test can hit a real `docling-serve` when
//! `DOCLING_URL` is set in the environment.

use std::path::PathBuf;

use httpmock::Method::POST;
use httpmock::MockServer;

// The `hybrid` module lives inside the `cnv` binary crate, so we can't
// `use cnv::hybrid::...` from integration tests. Instead, drive the hybrid
// path through the public CLI binary — that's what users actually hit.
use std::process::Command;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cnv"))
}

#[test]
fn hybrid_docling_uses_markdown_from_mock_server() {
    let server = MockServer::start();

    let canned = "# Canned Docling output\n\n\
                  $$ E = mc^2 $$\n\n\
                  This markdown came from the mock server.\n";

    let mock = server.mock(|when, then| {
        when.method(POST).path("/v1/convert/file");
        then.status(200)
            .header("content-type", "application/json")
            .body(format!(
                "{{\"document\": {{\"md_content\": {}}}}}",
                serde_json::to_string(canned).unwrap()
            ));
    });

    let root = project_root();
    let pdf = root.join("papers/golden/lorem.pdf");
    assert!(pdf.exists(), "fixture missing: {}", pdf.display());

    let out_dir = root.join("target/hybrid-mock-out");
    let _ = std::fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .arg(&pdf)
        .arg("-o")
        .arg(&out_dir)
        .arg("--hybrid")
        .arg("docling")
        .arg("--hybrid-url")
        .arg(server.base_url())
        // Force every page through the mock: lorem is plain prose, so the
        // Auto-policy triage would decline to route it.
        .arg("--hybrid-policy")
        .arg("all")
        .output()
        .expect("failed to invoke cnv");

    assert!(
        output.status.success(),
        "cnv --hybrid docling failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let md_path = out_dir.join("lorem/lorem.md");
    assert!(md_path.exists(), "expected output at {}", md_path.display());
    let content = std::fs::read_to_string(&md_path).unwrap();

    assert!(
        content.contains("Canned Docling output"),
        "output should include mock-server markdown; got:\n{content}",
    );
    assert!(
        content.contains("$$ E = mc^2 $$"),
        "display math from mock must round-trip verbatim",
    );

    mock.assert_hits(1);
}

#[test]
fn hybrid_docling_logs_backend_errors_and_keeps_local_output() {
    // Phase 2b semantics: per-page backend failures are logged to stderr and
    // the page keeps its locally-rendered output. The process exits 0 — the
    // user still got something useful for every page.
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST).path("/v1/convert/file");
        then.status(502)
            .header("content-type", "text/plain")
            .body("upstream busy");
    });

    let root = project_root();
    let pdf = root.join("papers/golden/lorem.pdf");
    let out_dir = root.join("target/hybrid-mock-err");
    let _ = std::fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .arg(&pdf)
        .arg("-o")
        .arg(&out_dir)
        .arg("--hybrid")
        .arg("docling")
        .arg("--hybrid-url")
        .arg(server.base_url())
        .arg("--hybrid-policy")
        .arg("all")
        .arg("--hybrid-timeout-secs")
        .arg("5")
        .output()
        .expect("failed to invoke cnv");

    assert!(
        output.status.success(),
        "cnv must exit 0 even when backend fails per-page — local output is the fallback"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("backend call failed") || stderr.contains("HTTP 502"),
        "expected backend-error message on stderr, got:\n{stderr}"
    );

    // Local path ran for lorem → we still got a non-empty markdown file.
    let md_path = out_dir.join("lorem/lorem.md");
    assert!(md_path.exists(), "local output should exist despite backend failure");
    let content = std::fs::read_to_string(&md_path).unwrap();
    assert!(!content.trim().is_empty(), "local fallback content should be non-empty");

    mock.assert_hits(1);
}

#[test]
fn hybrid_off_produces_same_output_as_before_phase_2() {
    // Regression guard: `--hybrid off` (default) must leave the local path
    // untouched. Compare against the Phase 1 snapshot committed in
    // tests/snapshots/attention_page_1.md.
    let root = project_root();
    let pdf = root.join("papers/attention.pdf");
    if !pdf.exists() {
        eprintln!("SKIP: {} missing", pdf.display());
        return;
    }

    let out_dir = root.join("target/hybrid-off-regression");
    let _ = std::fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .arg(&pdf)
        .arg("-o")
        .arg(&out_dir)
        // --hybrid defaults to off; assert by omission.
        .output()
        .expect("failed to invoke cnv");
    assert!(output.status.success(), "cnv failed: {}", String::from_utf8_lossy(&output.stderr));

    let md = std::fs::read_to_string(out_dir.join("attention/attention.md")).unwrap();
    let page_1 = md
        .split("<!-- page:2 -->")
        .next()
        .unwrap_or("")
        .trim_end();

    let snap = std::fs::read_to_string(root.join("tests/snapshots/attention_page_1.md"))
        .expect("Phase 1 snapshot should exist");
    assert_eq!(
        page_1.trim_end(),
        snap.trim_end(),
        "Phase 2 must not change `--hybrid off` output"
    );
}

#[test]
#[ignore = "requires a running docling-serve at $DOCLING_URL (default http://localhost:5001). \
            Run with: DOCLING_URL=http://localhost:5001 cargo test --test hybrid -- --ignored hybrid_live"]
fn hybrid_live() {
    let url = std::env::var("DOCLING_URL")
        .unwrap_or_else(|_| "http://localhost:5001".to_string());

    let root = project_root();
    let pdf = root.join("papers/math-number-theory.pdf");
    if !pdf.exists() {
        eprintln!("SKIP hybrid_live: {} missing", pdf.display());
        return;
    }

    let out_dir = root.join("target/hybrid-live");
    let _ = std::fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .arg(&pdf)
        .arg("-o")
        .arg(&out_dir)
        .arg("--hybrid")
        .arg("docling")
        .arg("--hybrid-url")
        .arg(&url)
        .arg("--hybrid-timeout-secs")
        .arg("600")
        .output()
        .expect("failed to invoke cnv");

    assert!(
        output.status.success(),
        "cnv --hybrid docling failed (is docling-serve up at {url}?): {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let md =
        std::fs::read_to_string(out_dir.join("math-number-theory/math-number-theory.md")).unwrap();

    assert!(
        md.contains("$$") || md.contains('$'),
        "expected at least one LaTeX math delimiter in Docling output for a math paper"
    );
}

