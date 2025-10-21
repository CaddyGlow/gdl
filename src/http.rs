use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use log::{debug, warn};
use reqwest::header::{ETAG, LAST_MODIFIED};
use reqwest::StatusCode;
use tokio::time::sleep;

use crate::cache::{load_cached_response, save_cached_response, CachedResponse};
use crate::rate_limit::RateLimitTracker;
use crate::utils::system_time_to_secs;

pub const DEFAULT_CACHE_TTL_SECS: u64 = 60 * 60; // 1 hour

pub async fn send_github_request_cached(
    builder: &reqwest::RequestBuilder,
    rate_limit: &Arc<RateLimitTracker>,
    context: &str,
    no_cache: bool,
) -> Result<Vec<u8>> {
    // Get the URL from the request builder for cache key
    let url = builder
        .try_clone()
        .ok_or_else(|| anyhow!("failed to clone request for cache lookup"))?
        .build()
        .context("failed to build request for cache lookup")?
        .url()
        .to_string();

    // Try to load from cache if caching is enabled
    let cached = if !no_cache {
        load_cached_response(&url, DEFAULT_CACHE_TTL_SECS)
            .ok()
            .flatten()
    } else {
        None
    };

    // If we have a valid cached response, use it directly without making any request
    // This avoids consuming GitHub API rate limit
    if let Some(cached_resp) = cached {
        debug!("Using cached response for {} (age: {}s, no request made)",
               url,
               system_time_to_secs(std::time::SystemTime::now()) - cached_resp.timestamp);
        return Ok(cached_resp.body);
    }

    // No valid cache, make a fresh request
    let request_builder = builder
        .try_clone()
        .ok_or_else(|| anyhow!("failed to clone GitHub request for {}", context))?;

    let response = send_github_request(&request_builder, rate_limit, context).await?;

    // Extract caching headers from response
    let headers = response.headers();
    let etag = headers
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let last_modified = headers
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Read response body
    let body = response
        .bytes()
        .await
        .with_context(|| format!("failed to read response body for {}", context))?
        .to_vec();

    // Cache the response if caching is enabled
    if !no_cache && (etag.is_some() || last_modified.is_some()) {
        let cached_response = CachedResponse {
            url: url.clone(),
            body: body.clone(),
            etag,
            last_modified,
            timestamp: system_time_to_secs(std::time::SystemTime::now()),
        };

        if let Err(e) = save_cached_response(&cached_response) {
            warn!("Failed to cache response for {}: {}", url, e);
        }
    }

    Ok(body)
}

pub async fn send_github_request(
    builder: &reqwest::RequestBuilder,
    rate_limit: &Arc<RateLimitTracker>,
    context: &str,
) -> Result<reqwest::Response> {
    const MAX_ATTEMPTS: usize = 5;

    for attempt in 1..=MAX_ATTEMPTS {
        let request = builder
            .try_clone()
            .ok_or_else(|| anyhow!("failed to clone GitHub request for {}", context))?;

        let response = request
            .send()
            .await
            .with_context(|| format!("GitHub request failed for {}", context))?;

        if let Some((snapshot, log_change, warn_low)) =
            rate_limit.record_headers(response.headers()).await
        {
            if log_change {
                debug!(
                    "GitHub rate limit: {} remaining of {} (used: {}) - resets {}",
                    snapshot
                        .remaining
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    snapshot
                        .limit
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    snapshot
                        .used
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    snapshot.reset_eta_display()
                );
            }

            if warn_low {
                warn!(
                    "GitHub rate limit low: {} remaining of {} (resets {}).",
                    snapshot
                        .remaining
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    snapshot
                        .limit
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    snapshot.reset_eta_display()
                );
            }
        }

        let status = response.status();
        if status.is_success() || status == StatusCode::NOT_MODIFIED {
            return Ok(response);
        }

        if let Some(wait) = RateLimitTracker::backoff_duration(status, response.headers()) {
            if attempt == MAX_ATTEMPTS {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<unable to read response body>".into());
                return Err(anyhow!(
                    "GitHub request {} exceeded rate limit after {} attempts (status {}): {}",
                    context,
                    attempt,
                    status,
                    body
                ));
            }

            let wait_secs = wait.as_secs().max(1);
            warn!(
                "GitHub request {} hit a rate limit (status {}), retrying after {}s...",
                context, status, wait_secs
            );
            sleep(wait).await;
            continue;
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read response body>".into());
        return Err(anyhow!(
            "GitHub request {} failed with status {}: {}",
            context,
            status,
            body
        ));
    }

    Err(anyhow!(
        "GitHub request {} failed after exhausting retries",
        context
    ))
}
