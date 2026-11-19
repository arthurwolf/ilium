//! Source request reservations shared by every scene and client of the plugin.
//!
//! A reservation is the wall-clock millisecond of the last request, kept in the plugin's
//! global storage bucket under `live-data/<name>`. A scene reserves before it contacts a provider, so a
//! failed attempt consumes the slot too, and unreadable or future-dated state fails closed
//! instead of resetting the budget.
use ilium_plugin_sdk::host::{Bucket, HostStorage};
use std::collections::HashMap;

/// Where reservations are kept.
pub trait Ledger: Send {
    fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, String>;
    fn write(&mut self, name: &str, value: Vec<u8>) -> Result<(), String>;
    fn compare_exchange(&mut self, name: &str, expected: Option<Vec<u8>>, value: Vec<u8>) -> Result<bool, String>;
}

/// The host's persistent, cross-session global bucket.
#[derive(Default)]
pub struct HostLedger;

fn key(name: &str) -> String {
    format!("live-data/{name}")
}

impl Ledger for HostLedger {
    fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let mut api = ilium_plugin_sdk::host();
        let storage_key = key(name);
        let current = HostStorage::get(&mut *api, Bucket::Global, storage_key.clone()).map_err(|error| error.message)?;
        if current.is_some() { return Ok(current); }
        let legacy = HostStorage::get(&mut *api, Bucket::Shared, storage_key.clone()).map_err(|error| error.message)?;
        let Some(value) = legacy else { return Ok(None) };
        if HostStorage::compare_exchange(&mut *api, Bucket::Global, storage_key.clone(), None, Some(value.clone())).map_err(|error| error.message)? {
            Ok(Some(value))
        } else {
            HostStorage::get(&mut *api, Bucket::Global, storage_key).map_err(|error| error.message)
        }
    }
    fn write(&mut self, name: &str, value: Vec<u8>) -> Result<(), String> {
        HostStorage::put(
            &mut *ilium_plugin_sdk::host(),
            Bucket::Global,
            key(name),
            value,
        )
        .map_err(|error| error.message)
    }
    fn compare_exchange(&mut self, name: &str, expected: Option<Vec<u8>>, value: Vec<u8>) -> Result<bool, String> {
        HostStorage::compare_exchange(&mut *ilium_plugin_sdk::host(), Bucket::Global, key(name), expected, Some(value))
            .map_err(|error| error.message)
    }
}

/// A ledger in memory, for tests and for scenes without storage.
#[derive(Default)]
pub struct MemoryLedger(pub HashMap<String, Vec<u8>>);

impl Ledger for MemoryLedger {
    fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.0.get(name).cloned())
    }
    fn write(&mut self, name: &str, value: Vec<u8>) -> Result<(), String> {
        self.0.insert(name.to_owned(), value);
        Ok(())
    }
    fn compare_exchange(&mut self, name: &str, expected: Option<Vec<u8>>, value: Vec<u8>) -> Result<bool, String> {
        if self.0.get(name).cloned() != expected { return Ok(false); }
        self.0.insert(name.to_owned(), value);
        Ok(true)
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
    for _ in 0..16 {
        let saved = ledger.read(name)?;
        if let Some(saved) = &saved {
            if saved.is_empty() || saved.len() > 64 {
                return Err("source request reservation is unreadable".into());
            }
            let previous = std::str::from_utf8(saved)
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
        if ledger.compare_exchange(name, saved, now_ms.to_string().into_bytes())? {
            return Ok(());
        }
    }
    Err("source request reservation changed repeatedly".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_sdk::testing::FakeHost;
    use ilium_plugin_sdk::types::{Activation, ErrorCode, Event, PluginError, Submission};
    use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

    #[ilium_plugin(side = "server", exports = ["lifecycle", "handler"])]
    #[derive(Default)]
    struct ReservationProbe;

    impl Lifecycle for ReservationProbe {
        fn activate(&mut self, _activation: Activation) -> Result<(), PluginError> {
            reserve(&mut HostLedger, "opensky", 1_000_000, 900_000)
                .map_err(|message| PluginError::new(ErrorCode::Io, message))
        }
    }

    impl Handler for ReservationProbe {
        fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
            Vec::new()
        }
    }

    #[test]
    fn unreadable_shared_reservation_fails_closed_and_can_be_retried() {
        let mut host = FakeHost::new();
        host.storage_mut().set_read_error(Some(PluginError::new(ErrorCode::Io, "storage unavailable")));
        assert!(host.try_activate::<ReservationProbe>().is_err());
        assert_eq!(host.storage().get(Bucket::Global, "live-data/opensky"), None);
        host.storage_mut().set_read_error(None);
        host.try_activate::<ReservationProbe>().expect("retry succeeds after storage recovers");
        assert_eq!(host.storage().get(Bucket::Global, "live-data/opensky"), Some(b"1000000".to_vec()));
    }

    #[test]
    fn legacy_shared_reservation_is_adopted_without_resetting_its_limit() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Shared, "live-data/opensky", b"900000");
        assert!(host.try_activate::<ReservationProbe>().is_err());
        assert_eq!(host.storage().get(Bucket::Global, "live-data/opensky"), Some(b"900000".to_vec()));
    }

    #[test]
    fn concurrent_reservation_is_reread_before_a_retry() {
        struct RacingLedger { value: Option<Vec<u8>>, raced: bool }
        impl Ledger for RacingLedger {
            fn read(&mut self, _name: &str) -> Result<Option<Vec<u8>>, String> { Ok(self.value.clone()) }
            fn write(&mut self, _name: &str, value: Vec<u8>) -> Result<(), String> { self.value = Some(value); Ok(()) }
            fn compare_exchange(&mut self, _name: &str, expected: Option<Vec<u8>>, value: Vec<u8>) -> Result<bool, String> {
                if !self.raced {
                    self.raced = true;
                    self.value = Some(b"1000000".to_vec());
                }
                if self.value != expected { return Ok(false); }
                self.value = Some(value);
                Ok(true)
            }
        }
        let mut ledger = RacingLedger { value: None, raced: false };
        assert!(reserve(&mut ledger, "opensky", 1_000_000, 900_000).is_err());
        assert_eq!(ledger.value, Some(b"1000000".to_vec()));
    }

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
