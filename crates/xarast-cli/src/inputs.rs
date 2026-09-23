//! Turning command line paths into the list of documents to process.

use std::path::{Path, PathBuf};

/// Expands the inputs: a file stands for itself, a directory for every
/// `.xar` file below it, recursively, in sorted order so that runs are
/// comparable.
///
/// # Errors
///
/// When a directory cannot be read.
pub fn expand(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for p in inputs {
        if p.is_dir() {
            let mut found = Vec::new();
            walk(p, &mut found)?;
            found.sort();
            out.extend(found);
        } else {
            out.push(p.clone());
        }
    }
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("xar"))
        {
            out.push(path);
        }
    }
    Ok(())
}

/// Milliseconds, for reports.
#[must_use]
pub fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
