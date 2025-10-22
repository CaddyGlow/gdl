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
