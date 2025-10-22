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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_snapshot_from_headers_complete() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-limit", "5000".parse().unwrap());
        headers.insert("x-ratelimit-remaining", "4999".parse().unwrap());
        headers.insert("x-ratelimit-used", "1".parse().unwrap());
        headers.insert("x-ratelimit-reset", "1234567890".parse().unwrap());

        let snapshot = RateLimitSnapshot::from_headers(&headers).unwrap();
        assert_eq!(snapshot.limit, Some(5000));
        assert_eq!(snapshot.remaining, Some(4999));
        assert_eq!(snapshot.used, Some(1));
        assert_eq!(snapshot.reset_epoch, Some(1234567890));
    }

    #[test]
    fn test_rate_limit_snapshot_from_headers_partial() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "100".parse().unwrap());

        let snapshot = RateLimitSnapshot::from_headers(&headers).unwrap();
        assert_eq!(snapshot.limit, None);
        assert_eq!(snapshot.remaining, Some(100));
        assert_eq!(snapshot.used, None);
        assert_eq!(snapshot.reset_epoch, None);
    }

    #[test]
    fn test_rate_limit_snapshot_from_headers_empty() {
        let headers = HeaderMap::new();
        assert!(RateLimitSnapshot::from_headers(&headers).is_none());
    }

    #[test]
    fn test_rate_limit_snapshot_reset_eta_display() {
        let snapshot = RateLimitSnapshot {
            limit: Some(5000),
            remaining: Some(4999),
            used: Some(1),
            reset_epoch: Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 120,
            ),
        };
        let display = snapshot.reset_eta_display();
        assert!(display.contains("in"));
        assert!(display.contains("s"));
    }

    #[test]
    fn test_rate_limit_snapshot_reset_eta_display_unknown() {
        let snapshot = RateLimitSnapshot {
            limit: Some(5000),
            remaining: Some(4999),
            used: Some(1),
            reset_epoch: None,
        };
        assert_eq!(snapshot.reset_eta_display(), "at an unknown time");
    }

    #[tokio::test]
    async fn test_rate_limit_tracker_record_headers() {
        let tracker = RateLimitTracker::default();
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-limit", "5000".parse().unwrap());
        headers.insert("x-ratelimit-remaining", "4999".parse().unwrap());
        headers.insert("x-ratelimit-used", "1".parse().unwrap());
        headers.insert("x-ratelimit-reset", "1234567890".parse().unwrap());

        let result = tracker.record_headers(&headers).await.unwrap();
        let (snapshot, log_change, warn_low) = result;

        assert_eq!(snapshot.limit, Some(5000));
        assert_eq!(snapshot.remaining, Some(4999));
        assert!(log_change); // First time should log
        assert!(!warn_low); // Not low yet
    }

    #[tokio::test]
    async fn test_rate_limit_tracker_warns_when_low() {
        let tracker = RateLimitTracker::default();
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-limit", "5000".parse().unwrap());
        headers.insert("x-ratelimit-remaining", "50".parse().unwrap());
        headers.insert("x-ratelimit-used", "4950".parse().unwrap());
        headers.insert("x-ratelimit-reset", "1234567890".parse().unwrap());

        let result = tracker.record_headers(&headers).await.unwrap();
        let (_, _, warn_low) = result;

        assert!(warn_low); // Should warn when at or below threshold
    }

    #[tokio::test]
    async fn test_rate_limit_tracker_no_duplicate_warnings() {
        let tracker = RateLimitTracker::default();
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-limit", "5000".parse().unwrap());
        headers.insert("x-ratelimit-remaining", "50".parse().unwrap());
        headers.insert("x-ratelimit-used", "4950".parse().unwrap());
        headers.insert("x-ratelimit-reset", "1234567890".parse().unwrap());

        // First time should warn
        let result1 = tracker.record_headers(&headers).await.unwrap();
        assert!(result1.2);

        // Second time with same remaining should NOT warn
        let result2 = tracker.record_headers(&headers).await.unwrap();
        assert!(!result2.2);
    }

    #[test]
    fn test_backoff_duration_too_many_requests() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "60".parse().unwrap());

        let duration = RateLimitTracker::backoff_duration(StatusCode::TOO_MANY_REQUESTS, &headers);
        assert_eq!(duration, Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_backoff_duration_forbidden_with_zero_remaining() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        headers.insert("retry-after", "30".parse().unwrap());

        let duration = RateLimitTracker::backoff_duration(StatusCode::FORBIDDEN, &headers);
        assert_eq!(duration, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_backoff_duration_forbidden_with_remaining() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "100".parse().unwrap());

        let duration = RateLimitTracker::backoff_duration(StatusCode::FORBIDDEN, &headers);
        assert_eq!(duration, None);
    }

    #[test]
    fn test_backoff_duration_success_status() {
        let headers = HeaderMap::new();
        let duration = RateLimitTracker::backoff_duration(StatusCode::OK, &headers);
        assert_eq!(duration, None);
    }

    #[test]
    fn test_header_value_to_u64_valid() {
        let mut headers = HeaderMap::new();
        headers.insert("x-test", "12345".parse().unwrap());
        assert_eq!(header_value_to_u64(&headers, "x-test"), Some(12345));
    }

    #[test]
    fn test_header_value_to_u64_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert("x-test", "not-a-number".parse().unwrap());
        assert_eq!(header_value_to_u64(&headers, "x-test"), None);
    }

    #[test]
    fn test_header_value_to_u64_missing() {
        let headers = HeaderMap::new();
        assert_eq!(header_value_to_u64(&headers, "x-missing"), None);
    }

    #[test]
    fn test_parse_retry_after_valid() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, "120".parse().unwrap());
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(120)));
    }

    #[test]
    fn test_parse_retry_after_missing() {
        let headers = HeaderMap::new();
        assert_eq!(parse_retry_after(&headers), None);
    }
}
