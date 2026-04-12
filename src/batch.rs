use std::path::{Path, PathBuf};
use glob::glob;
use crate::error::{VtvError, VtvResult};

/// Resolve the input string to a list of PDF file paths.
pub fn resolve_inputs(input: &str) -> VtvResult<Vec<PathBuf>> {
    let path = Path::new(input);

    // Single file
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("pdf") {
            return Ok(vec![path.to_path_buf()]);
        } else {
            return Err(VtvError::InvalidInput(
                input.to_string(),
                "not a PDF file".to_string(),
            ));
        }
    }

    // Directory — find all .pdf files
    if path.is_dir() {
        let mut pdfs: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(path).map_err(|e| VtvError::Io {
            path: path.to_path_buf(),
            source: e,
        })? {
            let entry = entry.map_err(|e| VtvError::Io {
                path: path.to_path_buf(),
                source: e,
            })?;
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("pdf") {
                pdfs.push(p);
            }
        }
        if pdfs.is_empty() {
            return Err(VtvError::InvalidInput(
                input.to_string(),
                "no PDF files found in directory".to_string(),
            ));
        }
        pdfs.sort();
        return Ok(pdfs);
    }

    // Glob pattern
    let matches: Vec<PathBuf> = glob(input)
        .map_err(|e| VtvError::InvalidInput(input.to_string(), e.to_string()))?
        .filter_map(|r| r.ok())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("pdf"))
        .collect();

    if matches.is_empty() {
        return Err(VtvError::InvalidInput(
            input.to_string(),
            "no matching PDF files found".to_string(),
        ));
    }

    Ok(matches)
}

/// Determine output directory for a given PDF input and optional base output dir.
pub fn output_dir_for(pdf_path: &Path, output_base: Option<&Path>) -> PathBuf {
    let stem = pdf_path.file_stem().unwrap_or_default().to_string_lossy();
    match output_base {
        Some(base) => base.join(stem.as_ref()),
        None => pdf_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(stem.as_ref()),
    }
}
