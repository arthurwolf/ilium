//! Source request reservations shared by every scene and client of the plugin.
//!
//! A reservation is the wall-clock millisecond of the last request, kept in the plugin's shared
//! storage bucket under `live-data/<name>`. A scene reserves before it contacts a provider, so a
//! failed attempt consumes the slot too, and unreadable or future-dated state fails closed
//! instead of resetting the budget.
use ilium_plugin_sdk::host::{Bucket, HostStorage};
use std::collections::HashMap;

/// Where reservations are kept.
pub trait Ledger: Send {
    fn read(&mut self, name: &str) -> Option<Vec<u8>>;
    fn write(&mut self, name: &str, value: Vec<u8>) -> Result<(), String>;
}

/// The host's shared storage bucket.
#[derive(Default)]
pub struct HostLedger;

fn key(name: &str) -> String {
    format!("live-data/{name}")
}

impl Ledger for HostLedger {
    fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        HostStorage::get(&mut *ilium_plugin_sdk::host(), Bucket::Shared, key(name))
    }
    fn write(&mut self, name: &str, value: Vec<u8>) -> Result<(), String> {
        HostStorage::put(
            &mut *ilium_plugin_sdk::host(),
            Bucket::Shared,
            key(name),
            value,
        )
        .map_err(|error| error.message)
    }
}

/// A ledger in memory, for tests and for scenes without storage.
#[derive(Default)]
pub struct MemoryLedger(pub HashMap<String, Vec<u8>>);

impl Ledger for MemoryLedger {
    fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        self.0.get(name).cloned()
    }
    fn write(&mut self, name: &str, value: Vec<u8>) -> Result<(), String> {
        self.0.insert(name.to_owned(), value);
        Ok(())
    }
}

/// Reserve before contacting the provider: failed attempts consume a slot too. Unknown or
/// corrupt state fails closed instead of resetting the budget.
pub fn reserve(
    ledger: &mut dyn Ledger,
    name: &str,
    now_ms: i64,
    interval_ms: i64,
) -> Result<(), String> {
    if now_ms < 0 || interval_ms <= 0 {
        return Err("invalid request reservation time".into());
    }
    if let Some(saved) = ledger.read(name) {
        if saved.is_empty() || saved.len() > 64 {
            return Err("source request reservation is unreadable".into());
        }
        let previous = std::str::from_utf8(&saved)
            .ok()
            .and_then(|text| text.trim().parse::<i64>().ok())
            .ok_or_else(|| "source request reservation is unreadable".to_owned())?;
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
    ledger.write(name, now_ms.to_string().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopen_and_clock_rollback_cannot_bypass_reservation() {
        let mut ledger = MemoryLedger::default();
        reserve(&mut ledger, "opensky", 1_000_000, 900_000).unwrap();
        assert!(reserve(&mut ledger, "opensky", 1_000_001, 900_000).is_err());
        assert!(reserve(&mut ledger, "opensky", 999_999, 900_000).is_err());
        reserve(&mut ledger, "opensky", 1_900_000, 900_000).unwrap();
        assert!(reserve(&mut ledger, "opensky", 1_900_001, 900_000).is_err());
    }

    #[test]
    fn corrupt_reservation_does_not_reset_budget() {
        let mut ledger = MemoryLedger::default();
        ledger.write("opensky", b"invalid".to_vec()).unwrap();
        assert!(reserve(&mut ledger, "opensky", 1_000_000, 900_000).is_err());
        ledger.write("opensky", vec![b'9'; 65]).unwrap();
        assert!(reserve(&mut ledger, "opensky", 1_000_000, 900_000).is_err());
    }

    #[test]
    fn sources_do_not_share_a_slot_and_a_refusal_names_the_wait() {
        let mut ledger = MemoryLedger::default();
        reserve(&mut ledger, "opensky", 1_000_000, 900_000).unwrap();
        reserve(&mut ledger, "digitraffic", 1_000_001, 30_000).unwrap();
        let error = reserve(&mut ledger, "opensky", 1_000_500, 900_000).unwrap_err();
        assert!(error.contains("wait 900 seconds"), "{error}");
    }

    #[test]
    fn invalid_time_is_refused() {
        let mut ledger = MemoryLedger::default();
        assert!(reserve(&mut ledger, "x", -1, 1000).is_err());
        assert!(reserve(&mut ledger, "x", 0, 0).is_err());
    }
}
