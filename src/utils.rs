use std::time::{Duration, SystemTime, UNIX_EPOCH};

use indicatif::MultiProgress;

/// Initialize logging with the specified verbosity level.
///
/// - 0: warn (default)
/// - 1: info
/// - 2: debug
/// - 3+: trace
///
/// Returns a MultiProgress instance for coordinating progress bars with logging
pub fn init_logging(verbosity: u8) -> MultiProgress {
    let default_level = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let multi = MultiProgress::new();

    let env = env_logger::Env::default().default_filter_or(default_level);
    let logger = env_logger::Builder::from_env(env)
        .format_timestamp_secs()
        .build();

    // Set up the log bridge so logs don't interfere with progress bars
    indicatif_log_bridge::LogWrapper::new(multi.clone(), logger)
        .try_init()
        .ok();

    multi
}

pub fn system_time_to_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn system_time_from_secs(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_time_to_secs() {
        let time = UNIX_EPOCH + Duration::from_secs(1234567890);
        assert_eq!(system_time_to_secs(time), 1234567890);
    }

    #[test]
    fn test_system_time_to_secs_epoch() {
        assert_eq!(system_time_to_secs(UNIX_EPOCH), 0);
    }

    #[test]
    fn test_system_time_from_secs() {
        let expected = UNIX_EPOCH + Duration::from_secs(987654321);
        assert_eq!(system_time_from_secs(987654321), expected);
    }

    #[test]
    fn test_system_time_from_secs_zero() {
        assert_eq!(system_time_from_secs(0), UNIX_EPOCH);
    }

    #[test]
    fn test_system_time_roundtrip() {
        let original_secs = 1609459200u64; // Jan 1, 2021 00:00:00 UTC
        let time = system_time_from_secs(original_secs);
        let recovered_secs = system_time_to_secs(time);
        assert_eq!(recovered_secs, original_secs);
    }
}
