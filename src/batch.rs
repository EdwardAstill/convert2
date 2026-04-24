use crate::cli::SUPPORTED_EXTENSIONS;
use crate::error::{VtvError, VtvResult};
use glob::glob;
use std::path::{Path, PathBuf};

/// Check if a file has a supported extension.
fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Resolve the input string to a list of PDF paths.
pub fn resolve_inputs(input: &str) -> VtvResult<Vec<PathBuf>> {
    let path = Path::new(input);

    // Single file
    if path.is_file() {
        if is_supported(path) {
            return Ok(vec![path.to_path_buf()]);
        } else {
            return Err(VtvError::InvalidInput(
                input.to_string(),
                format!(
                    "unsupported file type; supported extensions: {}",
                    SUPPORTED_EXTENSIONS.join(", ")
                ),
            ));
        }
    }

    // Directory — find all PDFs
    if path.is_dir() {
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(path).map_err(|e| VtvError::Io {
            path: path.to_path_buf(),
            source: e,
        })? {
            let entry = entry.map_err(|e| VtvError::Io {
                path: path.to_path_buf(),
                source: e,
            })?;
            let p = entry.path();
            if is_supported(&p) {
                files.push(p);
            }
        }
        if files.is_empty() {
            return Err(VtvError::InvalidInput(
                input.to_string(),
                "no supported files found in directory".to_string(),
            ));
        }
        files.sort();
        return Ok(files);
    }

    // Glob pattern
    let matches: Vec<PathBuf> = glob(input)
        .map_err(|e| VtvError::InvalidInput(input.to_string(), e.to_string()))?
        .filter_map(|r| r.ok())
        .filter(|p| is_supported(p))
        .collect();

    if matches.is_empty() {
        return Err(VtvError::InvalidInput(
            input.to_string(),
            "no matching supported files found".to_string(),
        ));
    }

    Ok(matches)
}

/// Determine output directory for a given input file and optional base output dir.
pub fn output_dir_for(file_path: &Path, output_base: Option<&Path>) -> PathBuf {
    let stem = file_path.file_stem().unwrap_or_default().to_string_lossy();
    match output_base {
        Some(base) => base.join(stem.as_ref()),
        None => file_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(stem.as_ref()),
    }
}
