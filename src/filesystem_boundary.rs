use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub fn ensure_same_filesystem(source: &Path, destination: &Path) -> Result<()> {
    let source_device = device_id(source)
        .with_context(|| format!("reading source filesystem for {}", source.display()))?;
    let destination_anchor = nearest_existing_ancestor(destination)?;
    let destination_device = device_id(&destination_anchor).with_context(|| {
        format!(
            "reading destination filesystem for {}",
            destination_anchor.display()
        )
    })?;

    if source_device != destination_device {
        bail!(
            "cross-filesystem mutation is not supported: source={} destination={} source_device={} destination_device={}",
            source.display(),
            destination.display(),
            source_device,
            destination_device
        );
    }

    Ok(())
}

fn nearest_existing_ancestor(path: &Path) -> Result<PathBuf> {
    let mut current = path.to_path_buf();
    loop {
        match fs::symlink_metadata(&current) {
            Ok(_) => return Ok(current),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let Some(parent) = current.parent() else {
                    bail!("no existing ancestor found for {}", path.display());
                };
                current = parent.to_path_buf();
            }
            Err(err) => return Err(err.into()),
        }
    }
}

#[cfg(unix)]
fn device_id(path: &Path) -> Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::symlink_metadata(path)?.dev())
}

#[cfg(not(unix))]
fn device_id(_path: &Path) -> Result<u64> {
    bail!("filesystem device identity is not supported on this platform")
}

#[cfg(test)]
mod tests {
    use super::{ensure_same_filesystem, nearest_existing_ancestor};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sandbox() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tidyfs-filesystem-boundary-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create sandbox");
        root
    }

    #[test]
    fn resolves_nearest_existing_destination_ancestor() {
        let root = sandbox();
        let destination = root.join("missing/nested/payload");
        assert_eq!(
            nearest_existing_ancestor(&destination).expect("resolve ancestor"),
            root
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn accepts_paths_on_the_same_filesystem() {
        let root = sandbox();
        let source = root.join("source");
        fs::write(&source, b"payload").expect("write source");
        ensure_same_filesystem(&source, &root.join("missing/payload"))
            .expect("same filesystem should pass");
        let _ = fs::remove_dir_all(root);
    }
}
