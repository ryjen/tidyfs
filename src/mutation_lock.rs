use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

pub struct MutationLock {
    _file: File,
}

impl MutationLock {
    pub fn acquire(db_path: &Path) -> Result<Self> {
        let path = lock_path(db_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating mutation lock directory {}", parent.display()))?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .with_context(|| format!("opening mutation lock {}", path.display()))?;

        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => bail!(
                "another tidyfs mutation is already running for database {}; lock={}",
                db_path.display(),
                path.display()
            ),
            Err(TryLockError::Error(err)) => Err(err)
                .with_context(|| format!("acquiring mutation lock {}", path.display())),
        }
    }
}

fn lock_path(db_path: &Path) -> PathBuf {
    let mut name = db_path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("tidyfs.db"));
    name.push(".mutation.lock");
    db_path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn lock_rejects_contention_and_releases_on_drop() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tidyfs-mutation-lock-{}-{nonce}",
            std::process::id()
        ));
        let db_path = root.join("state/tidyfs.db");

        assert_eq!(
            lock_path(&db_path),
            root.join("state/tidyfs.db.mutation.lock")
        );

        let first = MutationLock::acquire(&db_path).expect("acquire first mutation lock");
        let error = MutationLock::acquire(&db_path)
            .err()
            .expect("second mutation lock should be rejected");
        assert!(error.to_string().contains("another tidyfs mutation"));

        drop(first);
        MutationLock::acquire(&db_path).expect("lock should be released on drop");

        let _ = std::fs::remove_dir_all(root);
    }
}
