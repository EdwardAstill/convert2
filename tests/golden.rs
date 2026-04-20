//! Golden-output integration tests.
//!
//! Runs the built `cnv` binary against real PDFs and asserts on the output.
//!
//! Three test levels:
//!
//! 1. `golden_lorem_quick` — runs by default on `cargo test`. Tiny 10 KB PDF,
//!    < 1 s. Proves the binary is built and can produce non-empty markdown.
//!
//! 2. `golden_corpus_sweep` — `#[ignore]`. Iterates every PDF in the corpus,
//!    asserts exit 0, non-empty markdown, and that figure-heavy papers have
//!    ≥ 1 image extracted. Slow (~1–2 min). Run with:
//!    `cargo test --test golden -- --ignored`
//!
//! 3. `golden_snapshot_attention_page_1` — `#[ignore]`. Writes / diffs a
//!    snapshot of the first page of `attention.pdf`. First run writes the
//!    snapshot; subsequent runs fail on any diff. Regenerate with:
//!    `GOLDEN_UPDATE=1 cargo test --test golden -- --ignored`
//!
//! The binary path comes from the `CARGO_BIN_EXE_cnv` env var that Cargo sets
//! automatically for integration tests, so `cargo test` implicitly rebuilds
//! the bin before running these tests.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Every PDF expected to process end-to-end without errors.
const CORPUS_PATHS: &[&str] = &[
    "papers/attention.pdf",
    "papers/bert.pdf",
    "papers/clip.pdf",
    "papers/gpt3.pdf",
    "papers/resnet.pdf",
    "papers/math-number-theory.pdf",
    "papers/physics-hep.pdf",
    "papers/survey-llm.pdf",
    "papers/golden/lorem.pdf",
    "papers/golden/1901.03003.pdf",
    "papers/golden/2408.02509v1.pdf",
    "papers/golden/chinese_scan.pdf",
    "papers/golden/issue-336-conto-economico-bialetti.pdf",
];

/// PDFs that must yield at least one extracted image. ResNet is intentionally
/// omitted: its figures are mostly vector diagrams (residual blocks, CIFAR
/// plots drawn as paths), which mupdf reports as `Vector` blocks, not `Image`
/// blocks, and we currently only extract the latter.
const EXPECTS_IMAGES: &[&str] = &[
    "papers/attention.pdf",
    "papers/clip.pdf",
    "papers/survey-llm.pdf",
];

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cnv"))
}

fn run_cnv(pdf: &Path, out_dir: &Path) -> std::process::Output {
    Command::new(bin_path())
        .arg(pdf)
        .arg("-o")
        .arg(out_dir)
        .output()
        .expect("failed to execute cnv binary")
}

fn stem(pdf: &Path) -> String {
    pdf.file_stem().unwrap().to_string_lossy().into_owned()
}

#[test]
fn golden_lorem_quick() {
    let root = project_root();
    let pdf = root.join("papers/golden/lorem.pdf");
    if !pdf.exists() {
        eprintln!("SKIP golden_lorem_quick: no {}", pdf.display());
        return;
    }

    let out = root.join("target/golden-out-quick");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();

    let result = run_cnv(&pdf, &out);
    assert!(
        result.status.success(),
        "cnv failed on lorem.pdf: exit {:?}, stderr:\n{}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr)
    );

    let md_path = out.join("lorem/lorem.md");
    assert!(md_path.exists(), "expected markdown at {}", md_path.display());

    let content = std::fs::read_to_string(&md_path).unwrap();
    assert!(
        !content.trim().is_empty(),
        "lorem.md is empty — something is wrong in the happy path"
    );
}

#[test]
#[ignore = "slow (~1–2 min); heavy corpus sweep over 10+ PDFs. \
            Run with: cargo test --test golden -- --ignored"]
fn golden_corpus_sweep() {
    let root = project_root();
    let out = root.join("target/golden-out-corpus");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();

    let mut failures: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for rel in CORPUS_PATHS {
        let pdf = root.join(rel);
        if !pdf.exists() {
            skipped.push((*rel).to_string());
            continue;
        }

        let result = run_cnv(&pdf, &out);

        if !result.status.success() {
            failures.push(format!(
                "{rel}: exit {:?}\nstderr:\n{}",
                result.status.code(),
                String::from_utf8_lossy(&result.stderr)
            ));
            continue;
        }

        let s = stem(&pdf);
        let md_path = out.join(&s).join(format!("{s}.md"));
        if !md_path.exists() {
            failures.push(format!("{rel}: no markdown at {}", md_path.display()));
            continue;
        }

        let content = match std::fs::read_to_string(&md_path) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{rel}: read error: {e}"));
                continue;
            }
        };
        if content.trim().is_empty() {
            failures.push(format!("{rel}: empty markdown"));
            continue;
        }

        if EXPECTS_IMAGES.contains(rel) {
            let images_dir = out.join(&s).join("images");
            let img_count = std::fs::read_dir(&images_dir)
                .map(|iter| iter.count())
                .unwrap_or(0);
            if img_count == 0 {
                failures.push(format!(
                    "{rel}: expected ≥ 1 image in {}, got 0",
                    images_dir.display()
                ));
            }
        }
    }

    if !skipped.is_empty() {
        eprintln!("skipped {} missing fixture(s): {:?}", skipped.len(), skipped);
    }

    if !failures.is_empty() {
        panic!(
            "golden corpus sweep: {} failure(s):\n{}",
            failures.len(),
            failures.join("\n---\n")
        );
    }
}

#[test]
#[ignore = "requires attention.pdf; runs the full paper. \
            First run writes the snapshot; subsequent runs diff against it. \
            Regenerate with: GOLDEN_UPDATE=1 cargo test --test golden -- --ignored"]
fn golden_snapshot_attention_page_1() {
    let root = project_root();
    let pdf = root.join("papers/attention.pdf");
    if !pdf.exists() {
        eprintln!("SKIP: no {}", pdf.display());
        return;
    }

    let out = root.join("target/golden-out-snapshot");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();

    let result = run_cnv(&pdf, &out);
    assert!(
        result.status.success(),
        "cnv failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let md = std::fs::read_to_string(out.join("attention/attention.md"))
        .expect("attention.md should exist after successful run");

    // Slice the markdown to everything up to (but not including) the page-2
    // marker. This isolates the title / authors / abstract block, which is the
    // most reading-order-sensitive part of the document.
    let page_1 = md
        .split("<!-- page:2 -->")
        .next()
        .unwrap_or("")
        .trim_end()
        .to_string();

    let snap_path = root.join("tests/snapshots/attention_page_1.md");

    let regenerate = std::env::var("GOLDEN_UPDATE").is_ok();
    if regenerate || !snap_path.exists() {
        if let Some(parent) = snap_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&snap_path, &page_1).expect("write snapshot");
        eprintln!("wrote snapshot: {}", snap_path.display());
        return;
    }

    let expected = std::fs::read_to_string(&snap_path).expect("read snapshot");
    if expected.trim_end() != page_1.trim_end() {
        let actual_path = snap_path.with_extension("actual.md");
        std::fs::write(&actual_path, &page_1).ok();
        panic!(
            "attention.pdf page 1 snapshot mismatch.\n\
             expected: {}\n\
             actual:   {}\n\
             Inspect the diff, then regenerate with:\n\
             \tGOLDEN_UPDATE=1 cargo test --test golden -- --ignored",
            snap_path.display(),
            actual_path.display()
        );
    }
}
