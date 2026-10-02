//! Persisted source request reservations across scenes and Ilium processes.
use ilium_platform::{file_lock::ExclusiveFileLock, secure_fs};
use std::io::{Read, Write};
use std::path::Path;

/// Reserve before contacting the provider: failed attempts consume a slot
/// too. Unknown/corrupt state fails closed instead of resetting the budget.
pub fn reserve(path: &Path, now_ms: i64, interval_ms: i64) -> Result<(), String> {
    if now_ms < 0 || interval_ms <= 0 {
        return Err("invalid request reservation time".into());
    }
    let _lock = ExclusiveFileLock::try_acquire(&path.with_extension("lock"))
        .map_err(|error| error.to_string())?
        .ok_or("source request reservation busy; retry later")?;
    let mut saved = String::new();
    match secure_fs::private_open_options().read(true).open(path) {
        Ok(mut file) => {
            secure_fs::restrict_open_file_to_owner(&file).map_err(|error| error.to_string())?;
            Read::by_ref(&mut file)
                .take(65)
                .read_to_string(&mut saved)
                .map_err(|error| error.to_string())?;
            if saved.is_empty() || saved.len() > 64 {
                return Err("source request reservation is unreadable".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    if !saved.is_empty() {
        let previous = saved
            .trim()
            .parse::<i64>()
            .map_err(|_| "source request reservation is unreadable".to_owned())?;
        if previous < 0 {
            return Err("source request reservation is invalid".into());
        }
        let age = now_ms.saturating_sub(previous);
        if age < interval_ms {
            return Err(format!(
                "shared source request limit: wait {} seconds",
                (interval_ms.saturating_sub(age).saturating_add(999)) / 1000
            ));
        }
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{now_ms}.{unique}.tmp", std::process::id()));
    let mut created = false;
    let result = (|| {
        let mut file = secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        created = true;
        write!(file, "{now_ms}")?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() && created {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|error: std::io::Error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contended_reservation_fails_closed_without_contacting_provider() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source/request.time");
        let held = ExclusiveFileLock::acquire(&path.with_extension("lock")).unwrap();
        assert!(reserve(&path, 1_000_000, 900_000)
            .unwrap_err()
            .contains("busy"));
        assert!(!path.exists());
        drop(held);
        reserve(&path, 1_000_000, 900_000).unwrap();
    }

    #[test]
    fn reopen_and_clock_rollback_cannot_bypass_reservation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source/request.time");
        reserve(&path, 1_000_000, 900_000).unwrap();
        assert!(reserve(&path, 1_000_001, 900_000).is_err());
        assert!(reserve(&path, 999_999, 900_000).is_err());
        reserve(&path, 1_900_000, 900_000).unwrap();
        assert!(reserve(&path, 1_900_001, 900_000).is_err());
    }
    #[test]
    fn corrupt_reservation_does_not_reset_budget() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("request.time");
        std::fs::write(&path, b"invalid").unwrap();
        assert!(reserve(&path, 1_000_000, 900_000).is_err());
    }
}
