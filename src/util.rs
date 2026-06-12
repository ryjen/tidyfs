use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn resolve_db_path(path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = path {
        return Ok(expand_tilde(path));
    }

    if let Ok(path) = std::env::var("TIDYFS_DB") {
        return Ok(expand_tilde(PathBuf::from(path)));
    }

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set and --db was not provided")?;

    Ok(home.join(".local/share/tidyfs/tidyfs.db"))
}

pub fn normalize_existing_path(path: &Path) -> Result<PathBuf> {
    let expanded = expand_tilde(path.to_path_buf());
    expanded
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", expanded.display()))
}

pub fn normalize_path_best_effort(path: &Path) -> PathBuf {
    let expanded = expand_tilde(path.to_path_buf());
    expanded.canonicalize().unwrap_or(expanded)
}

pub fn expand_tilde(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();

    if s == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }

    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }

    path
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::format_bytes;

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
    }
}
