use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, RETRY_AFTER};
use reqwest::StatusCode;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitSnapshot {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub used: Option<u64>,
    pub reset_epoch: Option<u64>,
}

impl RateLimitSnapshot {
    pub fn from_headers(headers: &HeaderMap) -> Option<Self> {
        let limit = header_value_to_u64(headers, "x-ratelimit-limit");
        let remaining = header_value_to_u64(headers, "x-ratelimit-remaining");
        let used = header_value_to_u64(headers, "x-ratelimit-used");
        let reset_epoch = header_value_to_u64(headers, "x-ratelimit-reset");

        if limit.is_none() && remaining.is_none() && used.is_none() && reset_epoch.is_none() {
            return None;
        }

        Some(Self {
            limit,
            remaining,
            used,
            reset_epoch,
        })
    }

    pub fn reset_eta_display(&self) -> String {
        self.reset_epoch
            .and_then(|epoch| {
                let reset_time = UNIX_EPOCH + Duration::from_secs(epoch);
                reset_time.duration_since(SystemTime::now()).ok()
            })
            .map(|duration| format!("in {}s", duration.as_secs()))
            .unwrap_or_else(|| "at an unknown time".to_string())
    }
}

#[derive(Debug, Default)]
pub struct RateLimitState {
    pub last_snapshot: Option<RateLimitSnapshot>,
    pub lowest_remaining: Option<u64>,
    pub last_warned_remaining: Option<u64>,
}

#[derive(Debug, Default)]
pub struct RateLimitTracker {
    pub state: Mutex<RateLimitState>,
}

impl RateLimitTracker {
    pub async fn record_headers(
        &self,
        headers: &HeaderMap,
    ) -> Option<(RateLimitSnapshot, bool, bool)> {
        let snapshot = RateLimitSnapshot::from_headers(headers)?;
        let mut state = self.state.lock().await;

        let log_change = state
            .last_snapshot
            .as_ref()
            .map(|previous| previous != &snapshot)
            .unwrap_or(true);
        state.last_snapshot = Some(snapshot.clone());

        if let Some(remaining) = snapshot.remaining {
            state.lowest_remaining = Some(
                state
                    .lowest_remaining
                    .map_or(remaining, |lowest| lowest.min(remaining)),
            );
        }

        let warn_low = if let (Some(limit), Some(remaining)) = (snapshot.limit, snapshot.remaining)
        {
            let threshold = ((limit as f64) * 0.1).ceil() as u64;
            let threshold = threshold.max(50).min(limit);
            if remaining <= threshold {
                let should_warn = state
                    .last_warned_remaining
                    .map_or(true, |previous| remaining < previous);
                if should_warn {
                    state.last_warned_remaining = Some(remaining);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        drop(state);

        Some((snapshot, log_change, warn_low))
    }

    pub fn backoff_duration(status: StatusCode, headers: &HeaderMap) -> Option<Duration> {
        if status == StatusCode::TOO_MANY_REQUESTS {
            if let Some(duration) = parse_retry_after(headers) {
                return Some(duration);
            }
        }

        if status == StatusCode::FORBIDDEN {
            let remaining = header_value_to_u64(headers, "x-ratelimit-remaining");
            if let Some(remaining) = remaining {
                if remaining > 0 {
                    return None;
                }
            } else {
                return None;
            }
        } else if status != StatusCode::TOO_MANY_REQUESTS {
            return None;
        }

        if let Some(duration) = parse_retry_after(headers) {
            return Some(duration);
        }

        if let Some(reset_epoch) = header_value_to_u64(headers, "x-ratelimit-reset") {
            let reset_time = UNIX_EPOCH + Duration::from_secs(reset_epoch);
            if let Ok(duration) = reset_time.duration_since(SystemTime::now()) {
                if duration > Duration::from_secs(0) {
                    return Some(duration + Duration::from_secs(1));
                }
            }
        }

        None
    }
}

fn header_value_to_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|str_value| str_value.parse::<u64>().ok())
}

fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    value.parse::<u64>().ok().map(Duration::from_secs)
}
