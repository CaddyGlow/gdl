use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn init_logging(verbosity: u8) {
    let default_level = match verbosity {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    let env = env_logger::Env::default().default_filter_or(default_level);
    let _ = env_logger::Builder::from_env(env)
        .format_timestamp_secs()
        .try_init();
}

pub fn system_time_to_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn system_time_from_secs(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}
