//! Operating-system local time conversion for human-readable log columns.

use std::io;
use std::time::SystemTime;

/// Formats the machine's actual local civil time as `YYYY-MM-DD HH:mm:ss`.
/// Unsupported platforms and out-of-range times are explicit errors, never guessed UTC.
#[cfg(unix)]
pub fn format_timestamp(time: SystemTime) -> io::Result<String> {
    let seconds = match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(elapsed) => i128::from(elapsed.as_secs()),
        Err(error) => {
            let elapsed = error.duration();
            -i128::from(elapsed.as_secs()) - i128::from(elapsed.subsec_nanos() != 0)
        }
    };
    let seconds = libc::time_t::try_from(seconds)
        .map_err(|_| io::Error::other("timestamp is outside the platform time range"))?;
    let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: both pointers refer to valid objects for the full call; localtime_r
    // initializes `local` on success and does not retain either pointer.
    if unsafe { libc::localtime_r(&seconds, local.as_mut_ptr()) }.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful localtime_r initialized the entire tm value.
    let local = unsafe { local.assume_init() };
    let year = i64::from(local.tm_year) + 1900;
    if !(1..=9999).contains(&year) || !(0..60).contains(&local.tm_sec) {
        return Err(io::Error::other(
            "local timestamp cannot fit the log column",
        ));
    }
    Ok(format!(
        "{year:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.tm_mon + 1,
        local.tm_mday,
        local.tm_hour,
        local.tm_min,
        local.tm_sec
    ))
}

#[cfg(not(unix))]
pub fn format_timestamp(_time: SystemTime) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "local-time conversion is not implemented on this platform",
    ))
}
