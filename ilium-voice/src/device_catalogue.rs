//! Finite device-name preparation; native host allocations remain host-owned.

use std::fmt::{self, Display, Write};

use crate::VoiceError;

const MAX_DEVICES: usize = 256;
const MAX_DEVICE_NAME_BYTES: usize = 4 * 1024;
const MAX_DEVICE_TEXT_BYTES: usize = 64 * 1024;

struct DeviceName {
    text: String,
    limit: usize,
    refusal: Option<&'static str>,
}

impl Write for DeviceName {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let Some(length) = self
            .text
            .len()
            .checked_add(value.len())
            .filter(|length| *length <= self.limit)
        else {
            self.refusal = Some("device-name storage limit reached");
            return Err(fmt::Error);
        };
        if self
            .text
            .try_reserve_exact(length - self.text.len())
            .is_err()
        {
            self.refusal = Some("device-name allocation refused");
            return Err(fmt::Error);
        }
        if self.text.capacity() > self.limit {
            self.refusal = Some("device-name storage limit reached");
            return Err(fmt::Error);
        }
        self.text.push_str(value);
        Ok(())
    }
}

fn prepare_device_name(
    device: &impl Display,
    remaining: &mut usize,
    direction: &'static str,
) -> Result<String, VoiceError> {
    let refusal = |reason| VoiceError::AudioDeviceCatalogue { direction, reason };
    let mut name = DeviceName {
        text: String::new(),
        limit: (*remaining).min(MAX_DEVICE_NAME_BYTES),
        refusal: None,
    };
    if write!(&mut name, "{device}").is_err() || name.refusal.is_some() {
        return Err(refusal(
            name.refusal.unwrap_or("device name could not be formatted"),
        ));
    }
    *remaining -= name.text.capacity();
    Ok(name.text)
}

pub(crate) fn collect_device_names<T: Display>(
    devices: impl Iterator<Item = T>,
    direction: &'static str,
) -> Result<Vec<String>, VoiceError> {
    let mut names = Vec::new();
    let mut remaining = MAX_DEVICE_TEXT_BYTES;
    for (index, device) in devices.enumerate() {
        if index >= MAX_DEVICES {
            return Err(VoiceError::AudioDeviceCatalogue {
                direction,
                reason: "device count limit reached",
            });
        }
        names.push(prepare_device_name(&device, &mut remaining, direction)?);
    }
    names.sort_unstable();
    names.dedup();
    Ok(names)
}

/// Selects the original native owner in one pass; names are only temporary.
/// Charge aggregate formatting work even though preceding names are dropped.
pub(crate) fn find_named_device<T: Display>(
    devices: impl Iterator<Item = T>,
    requested_name: &str,
    direction: &'static str,
) -> Result<Option<T>, VoiceError> {
    if requested_name.len() > MAX_DEVICE_NAME_BYTES {
        return Err(VoiceError::AudioDeviceCatalogue {
            direction,
            reason: "requested device name exceeds storage limit",
        });
    }
    let mut remaining = MAX_DEVICE_TEXT_BYTES;
    for (index, device) in devices.enumerate() {
        if index >= MAX_DEVICES {
            return Err(VoiceError::AudioDeviceCatalogue {
                direction,
                reason: "device count limit reached",
            });
        }
        if prepare_device_name(&device, &mut remaining, direction)? == requested_name {
            return Ok(Some(device));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn lookup_returns_original_allocation_and_stops_at_first_match() {
        let first = Box::new("first".to_owned());
        let selected = Box::new("selected".to_owned());
        let original = &*selected as *const String;
        let visited = Cell::new(0);
        let devices = [first, selected, Box::new("later".to_owned())]
            .into_iter()
            .inspect(|_| visited.set(visited.get() + 1));
        let result = find_named_device(devices, "selected", "input")
            .unwrap()
            .unwrap();
        assert!(std::ptr::eq(&*result, original));
        assert_eq!(visited.get(), 2);
    }

    #[test]
    fn lookup_empty_and_missing_return_none() {
        assert!(
            find_named_device(std::iter::empty::<&str>(), "missing", "input")
                .unwrap()
                .is_none()
        );
        assert!(
            find_named_device(["other"].into_iter(), "missing", "output")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn lookup_refuses_infinite_scan_without_formatting_extra_device() {
        let formatted = Cell::new(0);
        let yielded = Cell::new(0);
        let devices = std::iter::from_fn(|| {
            yielded.set(yielded.get() + 1);
            Some(CountedName(&formatted))
        });
        assert!(matches!(
            find_named_device(devices, "missing", "input"),
            Err(VoiceError::AudioDeviceCatalogue {
                reason: "device count limit reached",
                ..
            })
        ));
        assert_eq!(formatted.get(), MAX_DEVICES);
        assert_eq!(yielded.get(), MAX_DEVICES + 1);
    }

    #[test]
    fn lookup_refuses_requested_name_before_advancing_iterator() {
        let visited = Cell::new(0);
        let devices = ["device"]
            .into_iter()
            .inspect(|_| visited.set(visited.get() + 1));
        assert!(matches!(
            find_named_device(devices, &"a".repeat(MAX_DEVICE_NAME_BYTES + 1), "output"),
            Err(VoiceError::AudioDeviceCatalogue {
                reason: "requested device name exceeds storage limit",
                ..
            })
        ));
        assert_eq!(visited.get(), 0);
    }

    #[test]
    fn lookup_charges_discarded_names_against_total_preparation_budget() {
        let name = "a".repeat(MAX_DEVICE_NAME_BYTES);
        let devices = std::iter::repeat_n(
            name.as_str(),
            MAX_DEVICE_TEXT_BYTES / MAX_DEVICE_NAME_BYTES + 1,
        );
        assert!(matches!(
            find_named_device(devices, "missing", "output"),
            Err(VoiceError::AudioDeviceCatalogue {
                reason: "device-name storage limit reached",
                ..
            })
        ));
    }

    #[test]
    fn preserves_sorted_unique_names() {
        assert_eq!(
            collect_device_names(["Zulu", "Alpha", "Zulu"].into_iter(), "input").unwrap(),
            ["Alpha", "Zulu"]
        );
    }

    struct CountedName<'a>(&'a Cell<usize>);
    impl Display for CountedName<'_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.set(self.0.get() + 1);
            formatter.write_str("device")
        }
    }

    #[test]
    fn stops_infinite_enumeration_without_formatting_extra_device() {
        let formatted = Cell::new(0);
        let yielded = Cell::new(0);
        let devices = std::iter::from_fn(|| {
            yielded.set(yielded.get() + 1);
            Some(CountedName(&formatted))
        });
        assert!(matches!(
            collect_device_names(devices, "output"),
            Err(VoiceError::AudioDeviceCatalogue {
                reason: "device count limit reached",
                ..
            })
        ));
        assert_eq!(formatted.get(), MAX_DEVICES);
        assert_eq!(yielded.get(), MAX_DEVICES + 1);
    }

    #[test]
    fn refuses_oversized_name_without_retaining_partial_catalogue() {
        let name = "a".repeat(MAX_DEVICE_NAME_BYTES + 1);
        assert!(matches!(
            collect_device_names(["valid", &name].into_iter(), "input"),
            Err(VoiceError::AudioDeviceCatalogue {
                reason: "device-name storage limit reached",
                direction: "input"
            })
        ));
    }

    #[test]
    fn shared_byte_limit_applies_even_to_duplicate_names() {
        let name = "a".repeat(MAX_DEVICE_NAME_BYTES);
        let devices = std::iter::repeat_n(
            name.as_str(),
            MAX_DEVICE_TEXT_BYTES / MAX_DEVICE_NAME_BYTES + 1,
        );
        assert!(matches!(
            collect_device_names(devices, "output"),
            Err(VoiceError::AudioDeviceCatalogue {
                reason: "device-name storage limit reached",
                ..
            })
        ));
    }

    #[test]
    fn exact_limits_are_accepted() {
        assert_eq!(
            collect_device_names(std::iter::repeat_n("", MAX_DEVICES), "input").unwrap(),
            [""]
        );
        let name = "a".repeat(MAX_DEVICE_NAME_BYTES);
        assert_eq!(
            collect_device_names(
                std::iter::repeat_n(name.as_str(), MAX_DEVICE_TEXT_BYTES / MAX_DEVICE_NAME_BYTES),
                "output"
            )
            .unwrap(),
            [name]
        );
    }

    #[test]
    fn writer_rejects_later_chunk_without_appending_it() {
        let mut name = DeviceName {
            text: String::new(),
            limit: 4,
            refusal: None,
        };
        name.write_str("abc").unwrap();
        assert!(name.write_str("de").is_err());
        assert_eq!(name.text, "abc");
        assert!(name.text.capacity() <= 4);
    }
}
